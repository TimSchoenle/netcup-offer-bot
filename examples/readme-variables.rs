//! Emit the half of the README payload that only this crate can answer.
//!
//! `README.md` and `docs/CONFIGURATION.md` are generated. The template holds the prose, this
//! holds the configuration facts derived from `Config`, and
//! `TimSchoenle/actions/actions/common/readme-variables` derives the rest from `Cargo.toml` and
//! `docs/`. That action reads this output through its `extra` input and deep-merges it, so
//! anything it already derives is deliberately absent here: the repository slug, the branch, the
//! release and the licence all reach the template as `repo.*` and `release.*`, and emitting a
//! second copy under another name would be a second thing to keep true.
//!
//! ```text
//! cargo run --quiet --features config-schema --example readme-variables
//! ```
//!
//! One line of strict JSON on stdout. `serde_json` does the escaping, so a generated Markdown
//! table full of `"`, `\` and newlines survives the trip through a workflow output verbatim —
//! the reason this is a Rust example and not a shell script assembling JSON with `printf`.
//!
//! It reads nothing from the environment, so it produces the same answer on a developer's
//! machine and on a runner where none of the variables it describes are set. Rendering is
//! deterministic for the same reason, which is what lets `.github/workflows/docs.yaml` verify a
//! committed `README.md` by re-rendering it.

use std::error::Error;
use std::process::ExitCode;

use netcup_offer_bot::config;
use serde::Serialize;
use terrace_config::schema::{Column, TomlExample};

/// The Docker Hub repository the image is published to.
///
/// Not derivable from the manifest: the namespace is a Docker Hub account rather than the
/// GitHub owner, and the two names differ.
const IMAGE: &str = "timmi6790/netcup-offer-bot";

/// What the templates reference and no manifest holds.
///
/// Strict mode is on in the workflow, so a template naming something neither this struct nor
/// the derived payload defines fails the render rather than silently emitting an empty table.
#[derive(Serialize)]
struct Variables {
    /// The Docker Hub repository, as `namespace/name`.
    image: &'static str,
    /// The prefix every configuration variable carries, e.g. `NETCUP_OFFER_BOT_`.
    prefix: String,
    /// What separates nesting levels in an environment key, e.g. `__`.
    nesting_separator: String,
    /// What marks a variable holding a path rather than a value, e.g. `_FILE`.
    indirection_suffix: String,
    /// The table of variables the loader reads before the layers exist.
    config_loader: String,
    /// The table of configuration keys.
    config_keys: String,
    /// A `config.toml` carrying every key, commented out where it has a default.
    config_toml: String,
}

fn main() -> ExitCode {
    match variables() {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("readme-variables: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Build the payload.
///
/// # Errors
/// Returns an error if the schema cannot be built or the payload cannot be encoded. Neither
/// depends on the machine this runs on, so either is a bug in the annotations on `Config`.
fn variables() -> Result<String, Box<dyn Error>> {
    let schema = config::schema()?;

    // No preamble and no per-key spellings: `docs/CONFIGURATION.md` states both, immediately
    // above this snippet and in far more detail than a comment in a file can. What is left is
    // what the section is for — the shape of the file, with every key in it.
    let example = TomlExample::new().header(false).spellings(false);

    let variables = Variables {
        image: IMAGE,
        prefix: schema.dialect.prefix.clone(),
        nesting_separator: schema.dialect.nesting_separator.clone(),
        indirection_suffix: schema.dialect.indirection_suffix.clone(),
        config_loader: block(&schema.to_markdown_loader()),
        config_keys: block(&schema.to_markdown_keys(Column::DEFAULT)),
        config_toml: block(&schema.to_toml_example_with(&example)),
    };

    Ok(serde_json::to_string(&variables)?)
}

/// One rendered block, without its trailing newline.
///
/// Every rendering ends with one so that appending the next needs no separator. A template does
/// its own spacing, so carrying the newline through would put a blank line after each block
/// that the template did not ask for and cannot remove.
fn block(rendered: &str) -> String {
    rendered.trim_end().to_owned()
}
