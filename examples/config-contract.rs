//! Emit the config contract, and the image labels that make it discoverable.
//!
//! ```text
//! cargo run --quiet --features config-schema --example config-contract -- --format contract    > docs/config.contract.json
//! cargo run --quiet --features config-schema --example config-contract -- --format labels      > contract.labels
//! cargo run --quiet --features config-schema --example config-contract -- --format dockerfile  # paste into the Dockerfile
//! ```
//!
//! Three renderings of one document, which is the point: the container build runs this twice in
//! a single stage — once for the contract and once for the labels — so the labels an image
//! carries and the document it publishes cannot describe different configurations. The
//! `dockerfile` rendering is the same labels as the `LABEL` instruction to paste, because a
//! `LABEL` key cannot be interpolated from anything and the block has to be written by hand.
//!
//! The sibling `readme-variables` example is the documentation half of the same schema. Neither
//! duplicates the other: that one renders tables for a human, this one renders a document for a
//! pipeline.
//!
//! This file is a command line and nothing else. The document is built by
//! [`netcup_offer_bot::config::contract`], which is in the library so that it can be tested.
//!
//! # Nothing is read from the environment
//!
//! Every rendering is a function of the source tree and the flags below, so this produces the
//! same bytes on a developer's machine, in a documentation job and in a container build. That is
//! what lets CI regenerate the committed `docs/config.contract.json` and diff it.
//!
//! # Why the release is a flag
//!
//! `App::version` is spelled the way the image tag spells it — `v2.0.1`, not `2.0.1` — and it is
//! **not** derived from `CARGO_PKG_VERSION`. It would go stale the moment release-please opened a
//! pull request bumping `Cargo.toml`: the committed contract would still name the old release and
//! the drift gate would fail the release pull request itself, every release, over a field that
//! has nothing to do with the configuration surface. Passing it leaves the container build as the
//! only place that states a release, which is the only place that knows one.
//!
//! `--revision` and `--created` are absent by default for a related reason: both make the
//! document non-reproducible across rebuilds of one commit.

use std::process::ExitCode;

use netcup_offer_bot::config::ConfigError;
use netcup_offer_bot::config::contract;
use terrace_config::schema::{Contract, DEFAULT_PATH};

fn main() -> ExitCode {
    let options = match Options::from_args() {
        Ok(options) => options,
        Err(message) => {
            eprintln!("config-contract: {message}");
            return ExitCode::FAILURE;
        }
    };

    match render(&options) {
        Ok(rendered) => {
            println!("{rendered}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("config-contract: {error}");
            ExitCode::FAILURE
        }
    }
}

fn render(options: &Options) -> Result<String, ConfigError> {
    let contract = contract(options)?;

    Ok(match options.format {
        Format::Contract => contract.to_json()?,
        Format::Labels => contract
            .labels(DEFAULT_PATH)
            .into_iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("\n"),
        Format::Dockerfile => contract
            .to_dockerfile_labels(DEFAULT_PATH)
            .trim_end()
            .to_owned(),
    })
}

/// The contract, with whatever this build was told about itself.
fn contract(options: &Options) -> Result<Contract, ConfigError> {
    let mut app = contract::app();
    if let Some(version) = &options.version {
        app = app.version(version);
    }
    if let Some(revision) = &options.revision {
        app = app.revision(revision);
    }
    if let Some(created) = &options.created {
        app = app.created(created);
    }
    contract::contract(app)
}

/// What to emit, and what this build knows about itself.
struct Options {
    format: Format,
    /// The release this build is of, spelled as the image tag spells it.
    version: Option<String>,
    /// The commit this build is of.
    revision: Option<String>,
    /// When this build happened, RFC 3339.
    created: Option<String>,
}

/// Which rendering to emit.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    /// The document the build embeds in its image and attaches to its digest.
    Contract,
    /// The image labels that make that document discoverable, one `NAME=value` per line.
    Labels,
    /// The same labels as the `LABEL` instruction to paste into the Dockerfile.
    Dockerfile,
}

impl Options {
    /// The contract itself, unless asked otherwise.
    fn from_args() -> Result<Self, String> {
        let mut options = Self {
            format: Format::Contract,
            version: None,
            revision: None,
            created: None,
        };

        let mut args = std::env::args().skip(1);
        while let Some(flag) = args.next() {
            match flag.as_str() {
                "--format" => {
                    options.format = match args.next().as_deref() {
                        Some("contract") => Format::Contract,
                        Some("labels") => Format::Labels,
                        Some("dockerfile") => Format::Dockerfile,
                        Some(other) => return Err(format!("unknown format `{other}`; {USAGE}")),
                        None => return Err(format!("--format takes a value; {USAGE}")),
                    };
                }
                "--version" => options.version = Some(value(&mut args, "--version takes a tag")?),
                "--revision" => {
                    options.revision = Some(value(&mut args, "--revision takes a commit")?);
                }
                "--created" => {
                    options.created = Some(value(&mut args, "--created takes a timestamp")?);
                }
                other => return Err(format!("unknown argument `{other}`; {USAGE}")),
            }
        }

        Ok(options)
    }
}

/// The next argument, or the caller's own message for why it had to be there.
fn value(args: &mut impl Iterator<Item = String>, expected: &str) -> Result<String, String> {
    args.next().ok_or_else(|| format!("{expected}; {USAGE}"))
}

const USAGE: &str = "usage: config-contract [--format contract|labels|dockerfile] \
                     [--version <tag>] [--revision <commit>] [--created <rfc3339>]";
