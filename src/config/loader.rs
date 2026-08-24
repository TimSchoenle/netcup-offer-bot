//! The netcup-offer-bot dialect of [`terrace_config`].
//!
//! The layering itself — the TOML layer, the prefixed environment layer, the secrets-directory
//! provider, the `_FILE` indirection and the shadow-key rejection — belongs to
//! `terrace-config`. What stays here is the one thing that is ours: which environment names
//! this deployment spells.
//!
//! Everything below goes through [`terrace`], so the loader that boots the process, the report
//! that explains it and the schema the README is generated from are the same dialect by
//! construction. A generator built over a second [`Terrace`] would document variables the
//! process does not read.

use serde::de::DeserializeOwned;
use terrace_config::Terrace;
use terrace_config::explain::Explanation;

pub use terrace_config::Error as ConfigError;

/// The prefix every configuration variable carries.
const PREFIX: &str = "NETCUP_OFFER_BOT_";

/// Builds the loader the process boots through.
///
/// Layers, lowest precedence first: struct defaults, TOML at `$NETCUP_OFFER_BOT_CONFIG` (a
/// file, or every `*.toml` in it if it names a directory), `NETCUP_OFFER_BOT_`-prefixed
/// `__`-nested environment variables, `$NETCUP_OFFER_BOT_SECRETS_DIR`, and
/// `NETCUP_OFFER_BOT_<KEY>_FILE` indirection. The last three are mutually exclusive per key: a
/// key supplied by two of them is refused at boot rather than resolved by precedence, so a
/// stale `NETCUP_OFFER_BOT_DISCORD__WEBHOOK_URL` cannot shadow a webhook that has since been
/// rotated in a mounted `Secret`.
///
/// Both variable names below are literals even though `Terrace::new(PREFIX)` derives exactly
/// these: naming them here is what puts them in the generated documentation table as the
/// deployment's own, rather than as a default a dependency happens to hold. Nothing is
/// reserved — this process reads no configuration key straight from the environment, so every
/// key is one a file may supply.
///
/// Public because the tests build their sandbox over it — `testing::Harness::over` derives
/// every name it writes from the loader it is handed, so a test cannot go on asserting a
/// variable this function has stopped naming.
#[must_use]
pub fn terrace() -> Terrace {
    Terrace::new(PREFIX)
        .config_var("NETCUP_OFFER_BOT_CONFIG")
        .secrets_dir_var("NETCUP_OFFER_BOT_SECRETS_DIR")
}

/// Loads a typed configuration.
///
/// # Errors
/// Returns [`ConfigError`] if a required value is missing, a value fails to parse, a
/// file-backed source cannot be read, or one key is supplied by more than one of the last
/// three layers.
pub fn load<T: DeserializeOwned>() -> Result<T, ConfigError> {
    terrace().load()
}

/// Reports which layer supplied each key, re-read at the moment it is called.
///
/// Holds no configuration value — not redacted on the way out, never recorded — so the report
/// is safe to log in full. It is what answers "the `Secret` is mounted and the bot is still
/// posting to the old webhook": the mount is listed, and so is the stale environment variable
/// sitting on top of it.
///
/// # Errors
/// Returns [`ConfigError`] if a layer cannot be read at all — an unreadable secrets directory,
/// or TOML that does not parse.
pub fn explain() -> Result<Explanation, ConfigError> {
    terrace().explain()
}

/// Describes the configuration surface as a schema, for the documentation job.
///
/// Reads nothing from the environment, so it produces the same answer on a runner where none
/// of the variables it describes are set.
#[cfg(feature = "config-schema")]
pub fn schema<T: terrace_config::schema::Describe + ?Sized>() -> terrace_config::schema::Schema {
    terrace().schema::<T>()
}
