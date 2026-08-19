#!/usr/bin/env bash
#
# The reverse gate: regenerate the config contract and the Dockerfile's `LABEL` block, and fail
# if either has drifted from what is committed.
#
#   .github/scripts/check-contract-drift.sh
#
# Run from the repository root, with a Rust toolchain on PATH. It rewrites
# `docs/config.contract.json` in place, so a local run doubles as the fix: inspect the diff,
# commit it.
#
# Why this exists when CI already checks the built image: this catches a renamed key, or a
# renamed prefix, in the pull request that renamed it — one step earlier than a label check that
# only runs once an image has been built, and in a diff a reviewer reads rather than in a build
# log nobody opens. It costs a `cargo run`.

set -euo pipefail

contract=docs/config.contract.json

generate() {
  cargo run --quiet --features config-schema --example config-contract -- "$@"
}

status=0

generate --format contract > "${contract}"
if ! git diff --exit-code --stat -- "${contract}"; then
  echo "error: ${contract} is stale — the configuration surface changed and the committed" >&2
  echo "       contract did not. The regenerated file is in your working tree; commit it." >&2
  echo >&2
  git --no-pager diff -- "${contract}" >&2
  status=1
fi

# The `LABEL` block is the one part of the contract that lives in a second file and cannot be
# generated into place: a `LABEL` key cannot be interpolated from anything. So it is compared
# instead.
#
# A bash substring test rather than `grep`: with `-z`, GNU grep still splits the *pattern* on
# newlines, so a three-line block matches an image that carries only the first line — which is
# precisely the drift this is here to catch. Carriage returns are stripped first, because the
# generator writes LF and a checkout on Windows may not.
block=$(generate --format dockerfile)
dockerfile=$(tr -d '\r' < Dockerfile)
if [[ "${dockerfile}" != *"${block}"* ]]; then
  echo "error: the Dockerfile does not carry the generated LABEL block verbatim. Paste this," >&2
  echo "       replacing the existing 'dev.terrace.config.*' block:" >&2
  echo >&2
  printf '%s\n' "${block}" >&2
  status=1
fi

if [ "${status}" -eq 0 ]; then
  echo "The committed contract and the Dockerfile's LABEL block are both current."
fi

exit "${status}"
