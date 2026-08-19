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
//! comment. Write that paragraph for an operator setting the value — anything below it stays
//! here, for whoever reads the type.

mod loader;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::str::FromStr;
use std::time::Duration;

use secrecy::SecretString;
use serde::{Deserialize, Deserializer};
use tracing::Level;

pub use loader::{ConfigError, explain, terrace};

const DEFAULT_METRIC_IP: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const DEFAULT_METRIC_PORT: u16 = 9184;
const DEFAULT_LOG_LEVEL: Level = Level::INFO;

/// Everything the process reads before it starts.
///
/// The two derives behind `config-schema` are the documentation job's: `Describe` reports the
/// keys and `Serialize` is what the generator reads the `Default` column out of. Neither has
/// another reason to exist here, since the loader only ever *deserialises* — and the
/// `#[config(...)]` attributes are gated the same way, because a helper attribute without the
/// derive that declares it is a compile error rather than a no-op.
#[derive(Debug, Deserialize)]
#[cfg_attr(
    feature = "config-schema",
    derive(serde::Serialize, terrace_config::schema::Describe)
)]
pub struct Config {
    #[cfg_attr(feature = "config-schema", config(nested))]
    pub discord: DiscordConfig,
    #[cfg_attr(feature = "config-schema", config(nested))]
    pub feed: FeedConfig,
    #[serde(default)]
    #[cfg_attr(feature = "config-schema", config(nested))]
    pub metrics: MetricsConfig,
    #[serde(default)]
    #[cfg_attr(feature = "config-schema", config(nested))]
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

/// The configuration surface as a schema, with its `Default` column filled in.
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
#[cfg_attr(
    feature = "config-schema",
    derive(serde::Serialize, terrace_config::schema::Describe)
)]
pub struct DiscordConfig {
    /// Discord webhook the offers are posted to.
    ///
    /// Secret: it is a bearer credential — anyone holding it can post to the channel — so it
    /// stays wrapped from the layer that read it to the request that uses it. `SecretString`
    /// refuses to implement `Serialize` for that reason, which is why the field is skipped on
    /// the way out; the key keeps its row and its `secret` flag either way, and a required key
    /// has no default to print.
    #[serde(skip_serializing)]
    #[cfg_attr(feature = "config-schema", config(secret))]
    pub webhook_url: SecretString,
}

/// How often the RSS feeds are polled.
#[derive(Debug, Deserialize)]
#[cfg_attr(
    feature = "config-schema",
    derive(serde::Serialize, terrace_config::schema::Describe)
)]
pub struct FeedConfig {
    /// Seconds between two RSS feed checks.
    ///
    /// Spelled in seconds rather than as a [`Duration`] so the TOML and the environment layer
    /// agree on one representation.
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
    /// The address the exporter binds.
    #[must_use]
    pub fn socket(&self) -> SocketAddr {
        SocketAddr::new(self.ip, self.port)
    }
}

/// Logging and error reporting.
#[derive(Debug, Deserialize)]
#[serde(default)]
#[cfg_attr(
    feature = "config-schema",
    derive(serde::Serialize, terrace_config::schema::Describe)
)]
pub struct TelemetryConfig {
    /// The maximum verbosity that reaches stdout: `TRACE`, `DEBUG`, `INFO`, `WARN` or `ERROR`,
    /// in any case.
    ///
    /// Parsed at boot so an unusable value fails the load rather than the first log line.
    /// `DEBUG` and `TRACE` additionally print which layer supplied each configuration key.
    #[serde(
        deserialize_with = "deserialize_level",
        serialize_with = "serialize_level"
    )]
    pub log_level: Level,
    /// Sentry DSN. Unset disables Sentry entirely.
    ///
    /// Secret: a DSN is a write credential for the project's event stream, and skipped on the
    /// way out for the reason [`DiscordConfig::webhook_url`] is. The key still reports itself
    /// as unset by default, which is the whole of what a table can usefully say about an
    /// optional secret.
    #[serde(skip_serializing)]
    #[cfg_attr(feature = "config-schema", config(secret))]
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

/// Render a [`Level`] the way every layer spells it.
///
/// The inverse of [`deserialize_level`], and reachable only from the schema generator: nothing
/// in this process serialises a `Config`, and `tracing::Level` has no `Serialize` impl of its
/// own for either of them to use.
#[cfg(feature = "config-schema")]
fn serialize_level<S>(level: &Level, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(level.as_str())
}
