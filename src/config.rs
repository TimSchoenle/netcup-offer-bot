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

mod loader;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::str::FromStr;
use std::time::Duration;

use secrecy::SecretString;
use serde::{Deserialize, Deserializer};
use tracing::Level;

pub use loader::ConfigError;

const DEFAULT_METRIC_IP: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const DEFAULT_METRIC_PORT: u16 = 9184;
const DEFAULT_LOG_LEVEL: Level = Level::INFO;

/// Everything the process reads before it starts.
#[derive(Debug, Deserialize)]
pub struct Config {
    pub discord: DiscordConfig,
    pub feed: FeedConfig,
    #[serde(default)]
    pub metrics: MetricsConfig,
    #[serde(default)]
    pub telemetry: TelemetryConfig,
}

impl Config {
    /// Load the configuration from every layer.
    ///
    /// # Errors
    /// Returns [`ConfigError`] if a required value is missing, a value fails to parse, a
    /// file-backed source cannot be read, or one key is supplied by more than one of the
    /// environment, the secrets directory and `_FILE` indirection.
    pub fn load() -> Result<Self, ConfigError> {
        loader::load()
    }
}

/// Where new offers are announced.
#[derive(Debug, Deserialize)]
pub struct DiscordConfig {
    /// The webhook to post to. Secret: it is a bearer credential — anyone holding it can post
    /// to the channel — so it stays wrapped from the layer that read it to the request that
    /// uses it.
    pub webhook_url: SecretString,
}

/// How often the RSS feeds are polled.
#[derive(Debug, Deserialize)]
pub struct FeedConfig {
    /// Seconds between two feed checks. Spelled in seconds rather than as a [`Duration`] so
    /// the TOML and the environment layer agree on one representation.
    check_interval_secs: u64,
}

impl FeedConfig {
    /// The poll interval as a [`Duration`].
    #[must_use]
    pub fn check_interval(&self) -> Duration {
        Duration::from_secs(self.check_interval_secs)
    }
}

/// Where the Prometheus exporter listens.
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct MetricsConfig {
    pub ip: IpAddr,
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
    /// The address the exporter binds.
    #[must_use]
    pub fn socket(&self) -> SocketAddr {
        SocketAddr::new(self.ip, self.port)
    }
}

/// Logging and error reporting.
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct TelemetryConfig {
    /// The maximum verbosity that reaches stdout, parsed at boot so an unusable value fails
    /// the load rather than the first log line.
    #[serde(deserialize_with = "deserialize_level")]
    pub log_level: Level,
    /// Sentry DSN. Absent disables Sentry entirely. Secret: a DSN is a write credential for
    /// the project's event stream.
    pub sentry_dsn: Option<SecretString>,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            log_level: DEFAULT_LOG_LEVEL,
            sentry_dsn: None,
        }
    }
}

/// Parse a [`Level`] from any layer's string form.
///
/// The error names the value and the accepted set, because the previous system's failure —
/// `LOG_LEVEL=FATAL`, a level `tracing` does not have — read only as "invalid level".
fn deserialize_level<'de, D>(deserializer: D) -> Result<Level, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    Level::from_str(&raw).map_err(|_| {
        serde::de::Error::custom(format!(
            "invalid log level `{raw}`, expected one of TRACE, DEBUG, INFO, WARN, ERROR"
        ))
    })
}
