//! The config contract, checked against the things it has to agree with.
//!
//! The assertions that matter are the two the deployment side depends on: the labels an image
//! carries must name the prefix the loader actually reads, and the offline copy's path must be
//! the one the build copies the document to. CI checks both against a *built image*, which is the
//! check that counts — a Dockerfile is a recipe and an image is what a registry serves. These
//! check the other half, that what the generator emits is right in the first place, and they do
//! it without Docker.
//!
//! Compiled only under `config-schema`, which is the feature the generator lives behind.

#![cfg(feature = "config-schema")]

use std::collections::BTreeMap;

use netcup_offer_bot::config;
use netcup_offer_bot::config::contract::{APP_NAME, SOURCE, app, contract};
use terrace_config::schema::{
    CONTRACT_VERSION, DEFAULT_PATH, LABEL_PATH, LABEL_PREFIX, LABEL_VERSION,
};

/// The prefix `src/config/loader.rs` gives the loader. Spelled out rather than imported: a test
/// that read the constant would pass just as happily after a rename, and stopping a rename from
/// reaching a deployment unannounced is this document's whole job.
const PREFIX: &str = "NETCUP_OFFER_BOT_";

#[test]
fn the_contract_builds() {
    let contract = contract(app()).expect("the contract should build");

    assert_eq!(contract.terrace_contract, CONTRACT_VERSION);
    assert_eq!(contract.app.name, APP_NAME);
    assert_eq!(contract.app.source.as_deref(), Some(SOURCE));
    // No release unless the caller states one. See the module docs on `config::contract::app`.
    assert_eq!(contract.app.version, None);
}

/// The agreement the chart repository checks from the other side: the image's `prefix` label must
/// equal the document's own `schema.dialect.prefix`. Rendering both from one [`Contract`] is what
/// makes that true, and this is the assertion that says so.
#[test]
fn the_labels_agree_with_the_document() {
    let contract = contract(app()).expect("the contract should build");
    let labels: BTreeMap<String, String> = contract
        .labels(DEFAULT_PATH)
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect();

    assert_eq!(
        labels.get(LABEL_PREFIX).map(String::as_str),
        Some(contract.schema.dialect.prefix.as_str())
    );
    assert_eq!(labels.get(LABEL_PREFIX).map(String::as_str), Some(PREFIX));
    assert_eq!(
        labels.get(LABEL_PATH).map(String::as_str),
        Some(DEFAULT_PATH)
    );
    assert_eq!(
        labels.get(LABEL_VERSION).map(String::as_str),
        Some(CONTRACT_VERSION.to_string().as_str())
    );

    // `verify_labels` is what a consumer runs against a built image. Running it here against the
    // labels the same contract emitted is not circular — it is the assertion that the generator's
    // two outputs are two halves of one document.
    contract
        .verify_labels(DEFAULT_PATH, &labels)
        .expect("the emitted labels should satisfy the contract that emitted them");
}

/// The `Dockerfile` cannot interpolate a `LABEL` key from anything, so the block is written by
/// hand and checked. CI checks the built image; this checks the recipe, which is where a reviewer
/// is looking.
#[test]
fn the_dockerfile_carries_the_generated_label_block() {
    let contract = contract(app()).expect("the contract should build");
    let block = contract.to_dockerfile_labels(DEFAULT_PATH);

    let dockerfile = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Dockerfile"))
        .expect("the Dockerfile should be readable")
        .replace('\r', "");

    assert!(
        dockerfile.contains(block.trim_end()),
        "the Dockerfile does not carry the generated LABEL block verbatim. Paste this, replacing \
         the existing `dev.terrace.config.*` block:\n\n{block}"
    );
}

/// The image copies the document to the path its own label names. A `COPY` to anywhere else
/// leaves the label pointing at nothing, which no consumer can distinguish from an image that
/// never carried a contract.
#[test]
fn the_dockerfile_copies_the_document_to_the_path_the_label_names() {
    let dockerfile = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Dockerfile"))
        .expect("the Dockerfile should be readable")
        .replace('\r', "");

    assert!(
        dockerfile.contains(&format!(
            "COPY --from=contract-builder /out/contract.json {DEFAULT_PATH}"
        )),
        "the runtime stage does not copy the contract to {DEFAULT_PATH}"
    );
}

/// Every documented key must be reachable from a Kubernetes `Secret` volume as well as from the
/// environment, because a mounted `Secret` is how the chart supplies the one credential this
/// process holds.
#[test]
fn every_key_has_an_environment_and_a_file_spelling() {
    let schema = config::schema().expect("the schema should describe");

    assert!(!schema.keys.is_empty(), "the schema describes no keys");
    for key in &schema.keys {
        assert!(
            key.env.is_some(),
            "`{}` has no environment spelling: {:?}",
            key.path,
            key.unreachable
        );
        assert!(
            key.secrets_file.is_some(),
            "`{}` cannot be supplied by a mounted Secret",
            key.path
        );
    }
}

/// The credential is marked secret, so nothing downstream renders its value into a document this
/// build publishes to a registry.
#[test]
fn the_webhook_is_secret_and_required_and_carries_no_default() {
    let schema = config::schema().expect("the schema should describe");
    let key = schema
        .keys
        .iter()
        .find(|key| key.path == "discord.webhook_url")
        .expect("`discord.webhook_url` should be described");

    assert!(key.secret, "the webhook is a bearer credential");
    assert!(key.required, "nothing can post without it");
    assert_eq!(key.default, None);
}

/// The defaults published in the document are the ones a process actually falls back to, read out
/// of the real `Default` implementations. This is also what exercises `serialize_level`, whose
/// output has to be a string the loader would accept back.
#[test]
fn the_published_defaults_are_the_ones_the_process_falls_back_to() {
    let contract = contract(app()).expect("the contract should build");
    let default_of = |path: &str| {
        contract
            .schema
            .keys
            .iter()
            .find(|key| key.path == path)
            .unwrap_or_else(|| panic!("`{path}` should be described"))
            .default
            .clone()
    };

    assert_eq!(default_of("metrics.ip").as_deref(), Some("127.0.0.1"));
    assert_eq!(default_of("metrics.port").as_deref(), Some("9184"));
    assert_eq!(default_of("telemetry.log_level").as_deref(), Some("INFO"));
}

/// A pod carries names no image asked for. Declaring them is what lets the contract keep its
/// default `unknown: reject` policy, which is the whole of the gate on the chart side — reaching
/// for `warn` instead would give it up to tolerate two.
#[test]
fn the_external_surface_accounts_for_what_a_pod_injects() {
    let contract = contract(app()).expect("the contract should build");

    assert!(
        contract.external.env.is_empty(),
        "this image reads nothing outside the loader's namespace"
    );
    assert!(contract.external.ignore.iter().any(|p| p == "KUBERNETES_*"));
    assert!(contract.external.ignore.iter().any(|p| p == "HOSTNAME"));
}
