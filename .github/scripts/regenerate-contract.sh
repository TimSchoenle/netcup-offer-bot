#!/usr/bin/env bash
#
# Rewrite everything derived from `src/config.rs`:
#
#   docs/config.contract.json   the contract the image publishes
#   Dockerfile                  the LABEL block that makes it discoverable
#
# A writer, not a checker. The checking half is the `rust/config-contract` action, which does it
# in three places this script never reached — against the Dockerfile, against the committed
# document, and against the labels a *built image* actually carries.
#
# Run it yourself before pushing, or let `docs.yml` run it on the pull request and commit the
# result: a pull request that renames a key or changes the loader's prefix then arrives with the
# document for it already written, in the commit that caused it.
#
# `--format contract` is generated without `--version`, `--revision` or `--created`, so it is
# byte-reproducible across rebuilds and releases: the committed copy describes the configuration
# surface, and the copy in an image additionally names the build it came from.

set -euo pipefail

if [ "$#" -ne 0 ]; then
    echo "usage: ${0##*/}" >&2
    exit 2
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"

contract="docs/config.contract.json"
dockerfile="Dockerfile"

# The crate's own markers, emitted by `--format dockerfile` along with the block between them.
# Delimiters rather than a pattern match: a Dockerfile carries other `LABEL` instructions, and
# cutting by line count reads correctly right up until a fourth label is added.
begin="# terrace-config:labels:begin"
end="# terrace-config:labels:end"

generate() {
    cargo run --quiet --features config-schema --example config-contract -- --format "$1"
}

mkdir -p "$(dirname "$contract")"
generate contract > "$contract"

if ! grep -qF "$begin" "$dockerfile" || ! grep -qF "$end" "$dockerfile"; then
    echo "error: $dockerfile carries no '$begin' … '$end' region, so the generated LABEL" \
         "instruction has nowhere to go. Paste '--format dockerfile' into it once." >&2
    exit 1
fi

block="$(mktemp)"
rewritten="$(mktemp)"
trap 'rm -f "$block" "$rewritten"' EXIT

generate dockerfile > "$block"

# `sed` cannot substitute a multi-line block portably, so the file is rewritten around the
# region instead: everything before `begin`, the generated block — which carries both markers —
# and everything after `end`.
{
    sed -n "1,/^${begin}\$/p" "$dockerfile" | sed '$d'
    cat "$block"
    sed -n "/^${end}\$/,\$p" "$dockerfile" | sed '1d'
} > "$rewritten"
mv "$rewritten" "$dockerfile"

echo "ok: rewrote $contract and the LABEL region in $dockerfile"
