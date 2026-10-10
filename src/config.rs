//! The typed configuration surface, and the loader every run boots through.
//!
//! The layering is [`terrace_config`]'s. Lowest precedence first: the `serde` defaults compiled
//! into the structs below, TOML at `$NETCUP_OFFER_BOT_CONFIG` (default `./config.toml`, absent
//! is not an error), `NETCUP_OFFER_BOT_`-prefixed `__`-nested environment variables, every
//! key-named file in `$NETCUP_OFFER_BOT_SECRETS_DIR`, and `NETCUP_OFFER_BOT_<KEY>_FILE`
//! indirection.
//!
//! The point of the last two is that the Discord webhook — the one credential this process
//! holds — can arrive as a mounted Kubernetes `Secret` or a Docker secret file rather than as
//! an environment variable that shows up in `docker inspect` and in every child process's
//! environment.
//!
//! Every layer spells a field the same way: `__` separates nesting levels and case is folded,
//! so `discord.webhook_url` is `NETCUP_OFFER_BOT_DISCORD__WEBHOOK_URL` as a variable and
//! `discord__webhook_url` as a file name.
//!
//! # The doc comments below are documentation output
//!
//! Under the `config-schema` feature these structs also derive `Describe`, and the
//! configuration tables in `README.md` are generated from what it reports: every key path,
//! every environment spelling, every default, and the *first paragraph* of each field's doc
//! comment. Write that paragraph for an operator setting the value. Anything below it stays
//! here, for whoever reads the type.
//!
// Gated rather than written as `//!`, because the link below is to an item that only exists
// under the feature and `rustdoc::broken_intra_doc_links` is denied. The section renders in the
// documentation job, which builds with `--all-features`.
#![cfg_attr(
    feature = "config-schema",
    doc = r"
# The contract this image publishes

Under the same feature, [`contract`] renders these types as the document the container build
embeds in the image and attaches to its pushed digest, and the `dev.terrace.config.*` labels
that make it discoverable. A key added below is a key the deployment side learns about in the
same commit; `docs/config.contract.json` is the committed copy, and CI fails a pull request
that changes one without the other.
"
)]

#[cfg(feature = "config-schema")]
pub mod contract;
mod level;
mod loader;
mod sentry;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Deserializer};

pub use level::{LogLevel, SentryLevel};
pub use loader::{ConfigError, explain, terrace};
pub use sentry::SentryConfig;

const DEFAULT_METRIC_IP: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const DEFAULT_METRIC_PORT: u16 = 9184;

/// Everything the process reads before it starts.
// Not rustdoc: the two derives behind `config-schema` are the documentation job's, and a caller
// of this type does nothing differently for knowing it. `Describe` reports the keys, `Serialize`
// is what the generator reads the `Default` column out of, and the loader itself only ever
// deserialises. The `#[config(...)]` attributes are gated the same way, because a helper
// attribute without the derive that declares it is a compile error and not a no-op.
#[derive(Debug, Deserialize)]
// A block nobody declared is a typo, and a typo that loads is the failure this refuses:
// `[telemtry]` would otherwise leave the process logging at `INFO` with no Sentry client and
// nothing said about why. Closing the *root* is safe because the loader keeps its own control
// variables out of the environment layer — `NETCUP_OFFER_BOT_CONFIG`,
// `NETCUP_OFFER_BOT_SECRETS_DIR` and every `_FILE` indirection are filtered before the merge, so
// none of them arrives here as a field this type never declared. Nothing is flattened into this
// struct, which is the one arrangement `deny_unknown_fields` cannot be combined with.
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "config-schema",
    derive(serde::Serialize, terrace_config::schema::Describe)
)]
pub struct Config {
    /// No default; the boot fails without a webhook.
    #[cfg_attr(feature = "config-schema", config(nested))]
    pub discord: DiscordConfig,
    /// Required. There is no default poll interval.
    #[cfg_attr(feature = "config-schema", config(nested))]
    pub feed: FeedConfig,
    /// Absent binds `127.0.0.1:9184`.
    #[serde(default)]
    #[cfg_attr(feature = "config-schema", config(nested))]
    pub metrics: MetricsConfig,
    /// Omitted, the process logs at `INFO` and stays out of Sentry.
    #[serde(default)]
    #[cfg_attr(feature = "config-schema", config(nested))]
    pub telemetry: TelemetryConfig,
}

impl Config {
    /// Loads the configuration from every layer.
    ///
    /// # Errors
    /// Returns [`ConfigError`] if a required value is missing, a value fails to parse, a
    /// file-backed source cannot be read, or one key is supplied by more than one of the
    /// environment, the secrets directory and `_FILE` indirection.
    pub fn load() -> Result<Self, ConfigError> {
        loader::load()
    }
}

/// The value the generated `Default` column is read out of, and nothing else.
///
/// Behind the feature because it is not a configuration this process could run on: the two
/// required keys have no meaningful default, and the schema skips them for exactly that
/// reason — a required key that printed a default would tell an operator they may leave it
/// out. Every value the table does show comes from the two blocks below, through the same
/// `Default` impls the loader itself falls back to.
///
/// Adding a block to [`Config`] fails to compile until it is added here too, which is what
/// keeps the column from quietly losing a row.
#[cfg(feature = "config-schema")]
impl Default for Config {
    fn default() -> Self {
        Self {
            discord: DiscordConfig {
                webhook_url: SecretString::from(String::new()),
            },
            feed: FeedConfig {
                check_interval_secs: 0,
            },
            metrics: MetricsConfig::default(),
            telemetry: TelemetryConfig::default(),
        }
    }
}

/// Renders the configuration surface as a schema, with its `Default` column filled in.
///
/// The generated half of `README.md`. It reads nothing from the environment, so it produces
/// the same answer on a developer's machine and on a runner where none of the variables it
/// describes are set.
///
/// # Errors
/// Returns [`ConfigError`] if [`Config::default`] cannot be serialised, which is a bug in the
/// annotations above rather than in anything an operator did.
#[cfg(feature = "config-schema")]
pub fn schema() -> Result<terrace_config::schema::Schema, ConfigError> {
    loader::schema::<Config>().with_defaults_from(&Config::default())
}

/// Where new offers are announced.
#[derive(Debug, Deserialize)]
// One key, and a misspelling of it is a boot without a webhook. See [`Config`].
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "config-schema",
    derive(serde::Serialize, terrace_config::schema::Describe)
)]
pub struct DiscordConfig {
    /// Discord webhook the offers are posted to.
    ///
    /// A bearer credential: anyone holding it can post to the channel. It stays wrapped from the
    /// layer that read it to the request that uses it, so nothing between the two can print it.
    /// Only an `https` URL loads; any other scheme fails the boot.
    // Not rustdoc: the paragraph above is rendered into the README table for an operator. Why the
    // field is skipped on the way out is for whoever changes this line: `SecretString` has no
    // `Serialize` impl, and the generated table loses nothing, since the key keeps its row and
    // its `secret` flag and a required key has no default to print.
    #[serde(skip_serializing, deserialize_with = "https_webhook_url")]
    #[cfg_attr(feature = "config-schema", config(secret))]
    pub webhook_url: SecretString,
}

/// Accepts the Discord webhook only as an absolute `https` URL.
///
/// The URL is the credential: its path carries the webhook token, so a request over any other
/// scheme sends the credential itself in the clear along with every offer posted. Refused here,
/// at load, so a mistyped `http://` fails the boot instead of leaking on the first post.
///
/// Neither error carries the value or any part of it. `url::ParseError` renders only the kind of
/// failure, and the scheme is left out on purpose: a value pasted without one can parse its host
/// as the scheme.
fn https_webhook_url<'de, D>(deserializer: D) -> Result<SecretString, D::Error>
where
    D: Deserializer<'de>,
{
    let webhook = SecretString::deserialize(deserializer)?;
    match reqwest::Url::parse(webhook.expose_secret()) {
        Ok(url) if url.scheme() == "https" => Ok(webhook),
        Ok(_) => Err(serde::de::Error::custom(
            "the Discord webhook must be an `https` URL",
        )),
        Err(error) => Err(serde::de::Error::custom(format_args!(
            "the Discord webhook is not a valid URL: {error}"
        ))),
    }
}

/// How often the RSS feeds are polled.
#[derive(Debug, Deserialize)]
// As [`Config`]: `check_intervall_secs` is a poll interval nobody set, not a default anybody
// chose.
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "config-schema",
    derive(serde::Serialize, terrace_config::schema::Describe)
)]
pub struct FeedConfig {
    /// Seconds between two RSS feed checks.
    ///
    /// Spelled in seconds rather than as a [`Duration`] so the TOML and the environment layer
    /// agree on one representation.
    // `0` is not a fast poll, it is a panic: `main` hands this to `tokio::time::interval`, which
    // is documented to panic on a zero period. The type's own `minimum: 0` therefore published a
    // value no build of this process can run on. An integer literal, matching the `u64`.
    #[cfg_attr(feature = "config-schema", config(range(min = 1)))]
    check_interval_secs: u64,
}

impl FeedConfig {
    /// Returns the poll interval as a [`Duration`].
    #[must_use]
    pub fn check_interval(&self) -> Duration {
        Duration::from_secs(self.check_interval_secs)
    }
}

/// Where the Prometheus exporter listens.
#[derive(Debug, Deserialize)]
// As [`Config`]: a misspelt key here binds the exporter somewhere nobody is scraping.
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "config-schema",
    derive(serde::Serialize, terrace_config::schema::Describe)
)]
pub struct MetricsConfig {
    /// Address the Prometheus exporter binds. `0.0.0.0` to reach it from outside the container.
    pub ip: IpAddr,
    /// Port the Prometheus exporter listens on.
    pub port: u16,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            ip: DEFAULT_METRIC_IP,
            port: DEFAULT_METRIC_PORT,
        }
    }
}

impl MetricsConfig {
    /// Returns the address the exporter binds.
    #[must_use]
    pub fn socket(&self) -> SocketAddr {
        SocketAddr::new(self.ip, self.port)
    }
}

/// Logging and error reporting.
#[derive(Debug, Default, Deserialize)]
// As [`Config`]. `sentry_dsn` below is *declared* rather than unknown, so it keeps reaching its
// own error message instead of the generic one this produces.
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "config-schema",
    derive(serde::Serialize, terrace_config::schema::Describe)
)]
#[allow(
    clippy::manual_non_exhaustive,
    reason = "`sentry_dsn` is a removed key kept to refuse it, not a marker sealing the struct"
)]
pub struct TelemetryConfig {
    /// The maximum verbosity that reaches stdout: `error`, `warn`, `info`, `debug` or `trace`.
    ///
    /// Read at boot, so a spelling this is not fails the load rather than the first log line.
    /// `debug` and `trace` additionally print which layer supplied each configuration key.
    // The variant list of [`LogLevel`] is the accepted set, and `config(values)` publishes that
    // same list: there is no parser between the two for them to disagree over.
    #[cfg_attr(feature = "config-schema", config(values))]
    pub log_level: LogLevel,
    /// Error reporting and performance tracing. Off unless configured; see [`SentryConfig`].
    ///
    /// Nested here rather than beside `metrics` because it is a second sink for the same
    /// `tracing` stream `log_level` governs, not a second exporter.
    #[serde(default)]
    #[cfg_attr(feature = "config-schema", config(nested))]
    pub sentry: SentryConfig,
    /// The key `telemetry.sentry.dsn` replaced. Supplying it fails the boot.
    ///
    /// A rename that resolved silently would take the deployment's error reporting away in the
    /// upgrade that renamed the key: the old variable would be ignored, `telemetry.sentry.enabled`
    /// would default to `false`, and the first anyone heard of it would be an incident nobody got
    /// an issue for. This field is what turns that into a boot failure naming the replacement.
    // Not rustdoc: `config(skip)` keeps it out of the schema. A key that exists only to be
    // refused is not part of the configuration surface a chart is checked against — the error
    // message is where it belongs, and it reaches the one person who set it.
    #[serde(skip_serializing, deserialize_with = "refuse_removed_sentry_dsn")]
    #[cfg_attr(feature = "config-schema", config(skip))]
    sentry_dsn: (),
}

/// Refuses the removed `telemetry.sentry_dsn` key, naming what replaced it.
///
/// Reached only when some layer supplied the key: `serde` skips a `deserialize_with` hook for an
/// absent field and takes the value from [`TelemetryConfig::default`] instead, so the cost of
/// this on every other boot is nothing.
///
/// The message states both halves of the migration. A DSN alone no longer switches Sentry on —
/// `telemetry.sentry.enabled` does — so an operator who moved only the value would land on the
/// same silent no-op one step later.
fn refuse_removed_sentry_dsn<'de, D>(deserializer: D) -> Result<(), D::Error>
where
    D: Deserializer<'de>,
{
    // Consumed rather than ignored: the value is a credential, and reading it into a `String`
    // to reject it would put it in the error `serde` renders for a type mismatch.
    serde::de::IgnoredAny::deserialize(deserializer)?;
    Err(serde::de::Error::custom(
        "`telemetry.sentry_dsn` was replaced by `telemetry.sentry.dsn`, and a DSN no longer \
         switches Sentry on by itself: set `telemetry.sentry.enabled` as well, then remove \
         `telemetry.sentry_dsn`",
    ))
}
