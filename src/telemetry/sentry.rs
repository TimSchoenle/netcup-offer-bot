//! The Sentry half of [`super::init`], in two arms.
//!
//! The crate is behind the `sentry` feature; the `telemetry.sentry` configuration block is not.
//! What follows is therefore written twice — once against the SDK, once against nothing — so
//! that [`super::init`] is one piece of code with no `#[cfg]` in it and the two builds differ
//! only in what they are able to do, not in how they are wired.
//!
//! The one behaviour the second arm has to get right is refusing `telemetry.sentry.enabled`.
//! Starting anyway would be the silent no-op the whole block is arranged to avoid, and it is
//! the failure mode an operator cannot see: a process that boots, serves and reports nothing
//! looks exactly like a quiet week.
//!
//! The extern crate is always spelled `::sentry`; the bare path is this module.

use crate::Result;
use crate::config::SentryConfig;
use crate::error::Error;

// ---------------------------------------------------------------------------------------------
// With the `sentry` feature: the client, and the layer that feeds it.
// ---------------------------------------------------------------------------------------------

/// What [`super::TelemetryGuard`] holds: the client's flush-on-drop guard, or nothing.
#[cfg(feature = "sentry")]
pub(super) type Guard = Option<::sentry::ClientInitGuard>;

/// Installs the process-wide client, or nothing when the block is switched off.
///
/// # Errors
/// Fails when `enabled` is set without a usable DSN, or with a sample rate outside
/// `0.0..=1.0`. Both are configuration mistakes whose only other outcome is a process that
/// reports nowhere.
#[cfg(feature = "sentry")]
pub(super) fn init(config: &SentryConfig) -> Result<Guard> {
    use secrecy::ExposeSecret as _;
    use std::time::Duration;

    if !config.enabled {
        return Ok(None);
    }

    // Empty is absent, not a value. An unfilled chart value and a compose pass-through both
    // resolve to an empty string, and both have to land on the message below rather than on the
    // parse error, which would send an operator looking at a URL that is not the problem.
    let dsn = config
        .dsn
        .as_ref()
        .map(|dsn| dsn.expose_secret().trim())
        .filter(|dsn| !dsn.is_empty());
    let Some(dsn) = dsn else {
        return Err(Error::Sentry(
            "telemetry.sentry.enabled is set but telemetry.sentry.dsn is empty; nothing would \
             be reported. Set the DSN, or turn the section off."
                .to_owned(),
        ));
    };
    // Parsed here rather than through `ClientOptions::dsn`, which panics on a malformed value.
    // The message deliberately does not quote the DSN: it is a credential, and this reaches the
    // log stream.
    let dsn = dsn.parse::<::sentry::types::Dsn>().map_err(|e| {
        Error::Sentry(format!(
            "telemetry.sentry.dsn is not a valid Sentry DSN ({e}); expected \
             https://<key>@<host>/<project>"
        ))
    })?;

    check_rate("sample_rate", config.sample_rate)?;
    check_rate("traces_sample_rate", config.traces_sample_rate)?;

    let environment = config
        .environment
        .clone()
        .unwrap_or_else(|| default_environment().to_owned());

    let mut options = ::sentry::ClientOptions::new()
        .debug(config.debug)
        .sample_rate(config.sample_rate)
        .traces_sample_rate(config.traces_sample_rate)
        .max_breadcrumbs(config.max_breadcrumbs)
        .attach_stacktrace(config.attach_stacktraces)
        .shutdown_timeout(Duration::from_secs(config.shutdown_timeout_secs))
        .environment(environment)
        // Marks this crate's own frames as application code, so a stack trace opens on the round
        // that failed rather than on a tokio internal.
        .in_app_include(vec!["netcup_offer_bot"]);
    options.dsn = Some(dsn);
    if let Some(release) = config
        .release
        .clone()
        .or_else(|| ::sentry::release_name!().map(std::borrow::Cow::into_owned))
    {
        options = options.release(release);
    }
    if let Some(server_name) = config.server_name.clone() {
        options = options.server_name(server_name);
    }

    // Every field `apply_defaults` would otherwise fill from `SENTRY_DSN`, `SENTRY_RELEASE` or
    // `SENTRY_ENVIRONMENT` is set above, and that is the point rather than thoroughness: those
    // variables are a second configuration channel that bypasses the layered loader and its
    // shadow-key rejection, and the config contract this image publishes declares that it reads
    // nothing outside the loader's namespace. An already-set field is one they cannot reach.
    Ok(Some(::sentry::init(options)))
}

/// The `tracing` layer feeding the client, or `None` when nothing would reach it.
///
/// Its level filter is the more verbose of the two thresholds, so a record below both is never
/// handed to the layer at all. `None` when the block is off, and also when both thresholds are
/// `off`: a layer that ignores everything still costs a callsite check on every record.
#[cfg(feature = "sentry")]
pub(super) fn tracing_layer<S>(
    config: &SentryConfig,
) -> Option<
    tracing_subscriber::filter::Filtered<
        ::sentry::integrations::tracing::SentryLayer<S>,
        tracing_subscriber::filter::LevelFilter,
        S,
    >,
>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    use ::sentry::integrations::tracing::{EventFilter, default_span_filter};
    use tracing_subscriber::Layer as _;
    use tracing_subscriber::filter::LevelFilter;

    if !config.enabled {
        return None;
    }
    let threshold = layer_threshold(config)?;

    let capture = config.capture_level;
    let breadcrumb = config.breadcrumb_level;
    // Nothing hands this process a trace to continue — it polls a feed and posts to a webhook —
    // so at rate `0.0` there is no inherited sampling decision that building spans anyway could
    // honour, and the sampler would discard every one of them. A service that *is* handed traces
    // must not gate span creation like this, or it cuts the trace at itself.
    let traces = config.traces_sample_rate > 0.0;

    let layer = ::sentry::integrations::tracing::layer()
        .event_filter(move |metadata| {
            let level = *metadata.level();
            if capture.accepts(level) {
                EventFilter::Event
            } else if breadcrumb.accepts(level) {
                EventFilter::Breadcrumb
            } else {
                EventFilter::Ignore
            }
        })
        .span_filter(move |metadata| traces && default_span_filter(metadata))
        .with_filter(LevelFilter::from_level(threshold));

    Some(layer)
}

/// Whether a client is installed.
#[cfg(feature = "sentry")]
pub(super) fn is_active(guard: &Guard) -> bool {
    guard.is_some()
}

/// The least severe level either sink accepts, or `None` when neither accepts anything.
///
/// [`tracing::Level`] orders `ERROR` lowest, so the more verbose of two thresholds is the
/// greater — the filter has to be the *looser* of the two, or the stricter sink silently
/// decides what the other one sees.
#[cfg(feature = "sentry")]
fn layer_threshold(config: &SentryConfig) -> Option<tracing::Level> {
    match (
        config.capture_level.threshold(),
        config.breadcrumb_level.threshold(),
    ) {
        (Some(capture), Some(breadcrumb)) => Some(capture.max(breadcrumb)),
        (capture, breadcrumb) => capture.or(breadcrumb),
    }
}

/// Refuses a rate outside `0.0..=1.0`.
///
/// # Errors
/// Fails naming the key and the value, because the two rates are the keys most likely to be
/// typed as a percentage.
#[cfg(feature = "sentry")]
fn check_rate(name: &str, rate: f32) -> Result<()> {
    if (0.0..=1.0).contains(&rate) {
        Ok(())
    } else {
        Err(Error::Sentry(format!(
            "telemetry.sentry.{name} must be between 0.0 and 1.0, got {rate}"
        )))
    }
}

/// The environment tag a build reports under when the configuration names none.
#[cfg(feature = "sentry")]
fn default_environment() -> &'static str {
    if cfg!(debug_assertions) {
        "development"
    } else {
        "production"
    }
}

// ---------------------------------------------------------------------------------------------
// Without it: the same three names, and a boot failure for the one configuration this build
// cannot honour.
// ---------------------------------------------------------------------------------------------

/// Nothing to hold: no client was linked.
///
/// A type of its own rather than `()`, so that [`super::init`] binds and moves it exactly as it
/// does the real guard. `()` makes the binding a unit value and the reference below a reference
/// to one, both of which clippy is right to object to in code that meant either.
#[cfg(not(feature = "sentry"))]
#[derive(Debug)]
pub(super) struct Guard;

/// Refuses `telemetry.sentry.enabled`, and otherwise does nothing.
///
/// # Errors
/// Fails when the block is switched on, naming the feature and both ways out. A binary that
/// cannot report has to say so at boot: the deployment asked for error reporting, and the
/// alternative is that it never learns it did not get any.
#[cfg(not(feature = "sentry"))]
pub(super) fn init(config: &SentryConfig) -> Result<Guard> {
    if config.enabled {
        return Err(Error::Sentry(
            "telemetry.sentry.enabled is set, but this binary was built without the `sentry` \
             feature and carries no client to install. Rebuild with default features, or set \
             telemetry.sentry.enabled = false."
                .to_owned(),
        ));
    }
    Ok(Guard)
}

/// No layer, whatever the configuration says.
///
/// [`tracing_subscriber::layer::Identity`] rather than a second shape for [`super::init`] to
/// compose: `None` of any layer type is a layer that does nothing, so the subscriber is built
/// the same way in both arms.
#[cfg(not(feature = "sentry"))]
pub(super) fn tracing_layer(_config: &SentryConfig) -> Option<tracing_subscriber::layer::Identity> {
    None
}

/// Never, in this build.
#[cfg(not(feature = "sentry"))]
pub(super) fn is_active(_guard: &Guard) -> bool {
    false
}

#[cfg(all(test, feature = "sentry"))]
mod tests {
    use super::{check_rate, layer_threshold, tracing_layer};
    use crate::config::{SentryConfig, SentryLevel};
    use tracing::Level;

    fn enabled() -> SentryConfig {
        SentryConfig {
            enabled: true,
            ..SentryConfig::default()
        }
    }

    /// The filter has to be the looser of the two thresholds. Taking the stricter one lets
    /// `capture_level = "error"` decide that no `INFO` record is ever kept as a breadcrumb,
    /// which empties the trail attached to every issue without changing a key anyone set.
    #[test]
    fn the_layer_filter_is_the_more_verbose_of_the_two_thresholds() {
        let config = SentryConfig {
            capture_level: SentryLevel::Error,
            breadcrumb_level: SentryLevel::Info,
            ..enabled()
        };
        assert_eq!(layer_threshold(&config), Some(Level::INFO));

        let config = SentryConfig {
            capture_level: SentryLevel::Warn,
            breadcrumb_level: SentryLevel::Off,
            ..enabled()
        };
        assert_eq!(layer_threshold(&config), Some(Level::WARN));

        let config = SentryConfig {
            capture_level: SentryLevel::Off,
            breadcrumb_level: SentryLevel::Trace,
            ..enabled()
        };
        assert_eq!(layer_threshold(&config), Some(Level::TRACE));
    }

    /// Both sinks off is not "report at the default level", and it is not a layer that ignores
    /// everything either — it is no layer.
    #[test]
    fn two_off_thresholds_install_no_layer() {
        let config = SentryConfig {
            capture_level: SentryLevel::Off,
            breadcrumb_level: SentryLevel::Off,
            ..enabled()
        };

        assert_eq!(layer_threshold(&config), None);
        assert!(tracing_layer::<tracing_subscriber::Registry>(&config).is_none());
    }

    /// The disabled path installs no layer at all — not a layer under a filter that happens to
    /// reject everything, which still runs on every record.
    #[test]
    fn the_disabled_block_installs_no_layer() {
        let config = SentryConfig::default();

        assert!(!config.enabled);
        assert!(tracing_layer::<tracing_subscriber::Registry>(&config).is_none());
    }

    #[test]
    fn a_rate_outside_the_unit_interval_is_refused() {
        assert!(check_rate("sample_rate", 0.0).is_ok());
        assert!(check_rate("sample_rate", 1.0).is_ok());

        let error = check_rate("traces_sample_rate", 20.0)
            .expect_err("a percentage typed as a fraction is refused");
        let message = error.to_string();
        assert!(
            message.contains("traces_sample_rate"),
            "must name the key: {message}"
        );
        assert!(message.contains("20"), "must name the value: {message}");
    }
}

/// The half of the disabled arm worth a test: the refusal, and that nothing else fails.
///
/// Run by `cargo test --no-default-features`, which is what `just test` adds a second pass for.
#[cfg(all(test, not(feature = "sentry")))]
mod tests {
    use super::init;
    use crate::config::SentryConfig;

    #[test]
    fn an_unconfigured_block_boots_a_build_with_no_client() {
        assert!(init(&SentryConfig::default()).is_ok());
    }

    #[test]
    fn enabling_it_without_a_client_is_a_boot_failure() {
        let config = SentryConfig {
            enabled: true,
            ..SentryConfig::default()
        };

        let error = init(&config).expect_err("this build has no client to install");
        let message = error.to_string();
        assert!(
            message.contains("sentry"),
            "must name the feature: {message}"
        );
    }
}
