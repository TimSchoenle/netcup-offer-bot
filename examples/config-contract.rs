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
//! # What is left here
//!
//! The `--format` vocabulary, the argument parsing, the dispatch, the printing and the exit code
//! are [`Cli`](terrace_config::schema::cli::Cli). They were the same program in every repository
//! that had a generator, which is how three of them ended up disagreeing about how to cut a
//! `LABEL` block back out of a Dockerfile.
//!
//! The document itself is built by [`contract`](config::contract::contract), which is in the
//! library so that it can be tested, from the same [`external`](config::contract::external)
//! surface handed to `Cli::contract_with` below.
//! One declaration, two callers, and no way for the document the tests check and the document
//! the build publishes to describe different external surfaces.
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

use netcup_offer_bot::config;
use terrace_config::schema::JsonSchema;
use terrace_config::schema::cli::Cli;

/// The `$id` the generated JSON Schema carries.
const SCHEMA_ID: &str = "https://github.com/TimSchoenle/netcup-offer-bot/config.schema.json";

fn main() -> ExitCode {
    let schema = match config::schema() {
        Ok(schema) => schema,
        Err(error) => {
            eprintln!("config-contract: {error}");
            return ExitCode::FAILURE;
        }
    };

    Cli::new(config::contract::app())
        .json_schema(
            JsonSchema::new()
                .title("netcup-offer-bot configuration")
                .id(SCHEMA_ID),
        )
        .contract_with(&|builder| builder.external(config::contract::external()))
        .main(schema)
}
