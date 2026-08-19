//! The config contract this image publishes.
//!
//! The document a chart is validated against: every configuration key in every spelling that can
//! supply it, the same keys as a JSON Schema, and the surface outside the loader's namespace. It
//! is built from [`schema`](super::schema), so it describes the [`Config`](super::Config) this
//! binary deserialises rather than a second account of it — and the three
//! `dev.terrace.config.*` labels an image carries are rendered from the same [`Contract`], which
//! is what stops a label from naming a prefix the loader does not read.
//!
//! Three carriers, one document. The labels live in the image config blob, so `crane config`
//! answers "does this image declare a contract" in one request with no layer pull; a copy at
//! [`DEFAULT_PATH`](terrace_config::schema::DEFAULT_PATH) makes the image self-describing with no
//! registry at all; and the canonical copy is attached to the pushed digest as an OCI referrer
//! and signed.
//!
//! This lives in the library rather than in `examples/config-contract.rs` because an example
//! cannot be tested. The example is a command line over it.
//!
//! Compiled only under `config-schema`; see the feature's comment in `Cargo.toml`.

use terrace_config::schema::{App, Contract, External};

use super::{ConfigError, schema};

/// The service's name, as its image and its chart name it.
pub const APP_NAME: &str = "netcup-offer-bot";

/// Where the source lives.
pub const SOURCE: &str = "https://github.com/TimSchoenle/netcup-offer-bot";

/// Which build a contract describes, before the caller states a release.
///
/// The release is the caller's to add, because this crate has no honest answer for it.
/// `CARGO_PKG_VERSION` looks like one and is not: it would go stale the moment release-please
/// opened a pull request bumping `Cargo.toml`, so the committed contract would disagree with the
/// source in exactly the pull request nobody wants to argue with. Only the container build knows
/// the tag it is being pushed under.
#[must_use]
pub fn app() -> App {
    App::new(APP_NAME).source(SOURCE)
}

/// The whole contract this image publishes: every configuration key, and everything else it
/// reads.
///
/// The external surface is two ignore patterns and no declared variables, and that is the honest
/// answer for this image rather than an unfinished list. The chart passes configuration as
/// files — `NETCUP_OFFER_BOT_CONFIG` and `NETCUP_OFFER_BOT_SECRETS_DIR`, both of which the loader
/// owns and the schema already describes — and the binary itself reads nothing from the
/// environment. What is left is what a Kubernetes pod carries whether an image asked for it or
/// not, which is what [`External::ignore`] is for.
///
/// # Errors
/// Returns [`ConfigError`] if the schema cannot be built, or if the crate refuses the declared
/// external surface. Neither depends on the machine this runs on: both are bugs in the
/// annotations on [`Config`](super::Config) or in this function.
pub fn contract(app: App) -> Result<Contract, ConfigError> {
    schema()?
        .into_contract(app)
        .external(External::new().ignore("KUBERNETES_*").ignore("HOSTNAME"))
        .build()
}
