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

/// The `tracing` layers feeding the client, or `None` when nothing would reach them.
///
/// Up to three layers, one per kind of record Sentry keeps, each behind a filter that is decided per
/// callsite from metadata alone, so a record no layer wants costs one cached interest check and
/// nothing per emission. `None` when the block is off, and also when both thresholds are `off`:
/// layers that ignore everything still cost a callsite check on every record.
#[cfg(feature = "sentry")]
pub(super) fn tracing_layer<S>(config: &SentryConfig) -> Option<impl tracing_subscriber::Layer<S>>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    use ::sentry::integrations::tracing::{BreadcrumbLayer, ErrorLayer, SpanLayer};
    use tracing_subscriber::Layer;
    use tracing_subscriber::filter::filter_fn;

    if !config.enabled {
        return None;
    }
    let routing = Routing::new(config)?;

    // First, as the SDK's migration guide orders them; and absent rather than filtered to
    // nothing when tracing is off, for the reason the `None` above gives.
    let spans = routing.traces.then(|| {
        SpanLayer::new().with_filter(
            filter_fn(move |metadata| metadata.is_span() && routing.is_span(*metadata.level()))
                .with_max_level_hint(routing.span_threshold()),
        )
    });
    // Breadcrumbs before errors, as the SDK requires for an event both accept to land in the
    // error's trail. Here no event reaches both — `Routing` sends each to exactly one — so the
    // order is the documented one rather than a load-bearing one.
    let breadcrumbs = BreadcrumbLayer::new().with_filter(
        filter_fn(move |metadata| metadata.is_event() && routing.is_breadcrumb(*metadata.level()))
            .with_max_level_hint(routing.threshold),
    );
    let errors = ErrorLayer::new().with_filter(
        filter_fn(move |metadata| metadata.is_event() && routing.is_error(*metadata.level()))
            .with_max_level_hint(routing.threshold),
    );

    // Spelled through the trait: `Option` has an inherent `and_then` that would win.
    Some(Layer::and_then(spans, breadcrumbs).and_then(errors))
}

/// Which Sentry sink a record at a given level reaches, decided once from the configuration.
///
/// Kept apart from the layers so the decision is testable without installing a subscriber, and
/// so the three filters above cannot disagree with each other about a threshold.
#[cfg(feature = "sentry")]
#[derive(Debug, Clone, Copy)]
struct Routing {
    capture: crate::config::SentryLevel,
    breadcrumb: crate::config::SentryLevel,
    /// The more verbose of the two thresholds: nothing below it reaches any sink.
    threshold: tracing::Level,
    /// Nothing hands this process a trace to continue — it polls a feed and posts to a webhook —
    /// so at rate `0.0` there is no inherited sampling decision that building spans anyway could
    /// honour, and the sampler would discard every one of them. A service that *is* handed
    /// traces must not gate span creation like this, or it cuts the trace at itself.
    traces: bool,
}

#[cfg(feature = "sentry")]
impl Routing {
    /// `None` when neither threshold accepts anything.
    fn new(config: &SentryConfig) -> Option<Self> {
        Some(Self {
            capture: config.capture_level,
            breadcrumb: config.breadcrumb_level,
            threshold: layer_threshold(config)?,
            traces: config.traces_sample_rate > 0.0,
        })
    }

    /// An event captured as a Sentry issue.
    fn is_error(self, level: tracing::Level) -> bool {
        self.capture.accepts(level)
    }

    /// An event kept as a breadcrumb: one the capture threshold did not already take, so no
    /// record is reported both as an issue and as a line in that issue's own trail.
    fn is_breadcrumb(self, level: tracing::Level) -> bool {
        !self.capture.accepts(level) && self.breadcrumb.accepts(level)
    }

    /// A span recorded into a transaction: `INFO` or more severe, and within the threshold.
    fn is_span(self, level: tracing::Level) -> bool {
        self.traces && level <= self.span_threshold()
    }

    /// The least severe span level recorded. Capped at `INFO`, the span levels the SDK's legacy
    /// combined layer recorded by default, so the migration off it changes no transaction.
    fn span_threshold(self) -> tracing::Level {
        self.threshold.min(tracing::Level::INFO)
    }
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
    use super::{Routing, check_rate, layer_threshold, tracing_layer};
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
    fn the_threshold_is_the_more_verbose_of_the_two_levels() {
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

    /// Each event reaches exactly one sink: an issue, or a breadcrumb, never both. A record kept
    /// as both would appear as the last line of its own issue's trail.
    #[test]
    fn an_event_is_routed_to_exactly_one_sink() {
        let routing = Routing::new(&SentryConfig {
            capture_level: SentryLevel::Warn,
            breadcrumb_level: SentryLevel::Debug,
            ..enabled()
        })
        .expect("both thresholds accept something");

        for level in [Level::ERROR, Level::WARN] {
            assert!(routing.is_error(level), "{level} is captured");
            assert!(
                !routing.is_breadcrumb(level),
                "{level} is not also a breadcrumb"
            );
        }
        for level in [Level::INFO, Level::DEBUG] {
            assert!(!routing.is_error(level), "{level} is not captured");
            assert!(routing.is_breadcrumb(level), "{level} is a breadcrumb");
        }
        assert!(!routing.is_error(Level::TRACE));
        assert!(!routing.is_breadcrumb(Level::TRACE));
    }

    /// Spans are recorded only with a non-zero trace rate, never below `INFO`, and never below
    /// the looser of the two thresholds.
    #[test]
    fn spans_follow_the_trace_rate_and_both_caps() {
        let routing = |traces_sample_rate, breadcrumb_level| {
            Routing::new(&SentryConfig {
                traces_sample_rate,
                capture_level: SentryLevel::Error,
                breadcrumb_level,
                ..enabled()
            })
            .expect("thresholds accept something")
        };

        let traced = routing(0.5, SentryLevel::Trace);
        assert!(traced.is_span(Level::INFO));
        assert!(!traced.is_span(Level::DEBUG), "capped at INFO");

        let strict = routing(0.5, SentryLevel::Warn);
        assert!(strict.is_span(Level::WARN));
        assert!(!strict.is_span(Level::INFO), "capped at the threshold");

        let untraced = routing(0.0, SentryLevel::Trace);
        assert!(!untraced.is_span(Level::ERROR), "no spans at rate 0.0");
    }

    /// Collects every envelope the client would have sent.
    #[derive(Default)]
    struct Collect(std::sync::Mutex<Vec<::sentry::protocol::Envelope>>);

    impl ::sentry::Transport for Collect {
        fn send_envelope(&self, envelope: ::sentry::protocol::Envelope) {
            self.0
                .lock()
                .expect("no test thread panics holding the lock")
                .push(envelope);
        }
    }

    /// What the installed layers send for one `INFO` span holding one event at each level, with
    /// `capture_level = "error"` and `breadcrumb_level = "info"`.
    fn report(traces_sample_rate: f32) -> Vec<::sentry::protocol::EnvelopeItem> {
        use std::sync::Arc;
        use tracing_subscriber::layer::SubscriberExt as _;

        let transport = Arc::new(Collect::default());
        let mut options = ::sentry::ClientOptions::new()
            .traces_sample_rate(traces_sample_rate)
            .transport(Arc::clone(&transport));
        options.dsn = Some(
            "https://public@sentry.invalid/1"
                .parse()
                .expect("a valid DSN"),
        );
        let hub = Arc::new(::sentry::Hub::new(
            Some(Arc::new(options.into())),
            Arc::new(::sentry::Scope::default()),
        ));
        let config = SentryConfig {
            capture_level: SentryLevel::Error,
            breadcrumb_level: SentryLevel::Info,
            traces_sample_rate,
            ..enabled()
        };
        let layer = tracing_layer::<tracing_subscriber::Registry>(&config)
            .expect("an enabled block with live thresholds installs a layer");
        let subscriber = tracing_subscriber::registry().with(layer);

        ::sentry::Hub::run(hub, || {
            tracing::subscriber::with_default(subscriber, || {
                let _round = tracing::info_span!("round").entered();
                tracing::debug!("debug");
                tracing::info!("info");
                tracing::warn!("warn");
                tracing::error!("error");
            });
        });

        transport
            .0
            .lock()
            .expect("no test thread panics holding the lock")
            .drain(..)
            .flat_map(::sentry::protocol::Envelope::into_items)
            .collect()
    }

    /// The wiring, end to end: one issue for the one `ERROR`, carrying the `INFO` and `WARN`
    /// records as its trail and neither the `DEBUG` record nor itself.
    #[test]
    fn each_record_reaches_the_sink_its_level_selects() {
        use ::sentry::protocol::{EnvelopeItem, Level as SentryEventLevel};

        let items = report(0.0);
        let events: Vec<_> = items
            .iter()
            .filter_map(|item| match item {
                EnvelopeItem::Event(event) => Some(event),
                _ => None,
            })
            .collect();
        assert_eq!(events.len(), 1, "exactly the ERROR record is an issue");
        let event = events[0];
        assert_eq!(event.level, SentryEventLevel::Error);

        let trail: Vec<_> = event
            .breadcrumbs
            .values
            .iter()
            .map(|crumb| crumb.message.as_deref())
            .collect();
        assert_eq!(trail, [Some("info"), Some("warn")]);

        assert!(
            !items
                .iter()
                .any(|item| matches!(item, EnvelopeItem::Transaction(_))),
            "no transaction at rate 0.0"
        );
    }

    /// With a non-zero trace rate the `INFO` span becomes a transaction.
    #[test]
    fn a_traced_span_becomes_a_transaction() {
        use ::sentry::protocol::EnvelopeItem;

        let items = report(1.0);
        let transactions: Vec<_> = items
            .iter()
            .filter_map(|item| match item {
                EnvelopeItem::Transaction(transaction) => Some(transaction),
                _ => None,
            })
            .collect();
        assert_eq!(transactions.len(), 1);
        assert_eq!(transactions[0].name.as_deref(), Some("round"));
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
