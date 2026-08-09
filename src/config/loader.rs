//! The netcup-offer-bot dialect of [`terrace_config`].
//!
//! The layering itself — the TOML layer, the prefixed environment layer, the secrets-directory
//! provider, the `_FILE` indirection and the shadow-key rejection — belongs to
//! `terrace-config`. What stays here is the one thing that is ours: which environment names
//! this deployment spells.

use serde::de::DeserializeOwned;
use terrace_config::Terrace;

pub use terrace_config::Error as ConfigError;

/// The prefix every configuration variable carries.
const PREFIX: &str = "NETCUP_OFFER_BOT_";

/// The loader the process boots through.
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
/// these: the README documents them, and a name that exists only as a derivation inside a
/// dependency is one no line of documentation can be held to. Nothing is reserved — this
/// process reads no configuration key straight from the environment, so every key is one a
/// file may supply.
fn terrace() -> Terrace {
    Terrace::new(PREFIX)
        .config_var("NETCUP_OFFER_BOT_CONFIG")
        .secrets_dir_var("NETCUP_OFFER_BOT_SECRETS_DIR")
}

/// Load a typed configuration.
///
/// # Errors
/// Returns [`ConfigError`] if a required value is missing, a value fails to parse, a
/// file-backed source cannot be read, or one key is supplied by more than one of the last
/// three layers.
pub fn load<T: DeserializeOwned>() -> Result<T, ConfigError> {
    terrace().load()
}
