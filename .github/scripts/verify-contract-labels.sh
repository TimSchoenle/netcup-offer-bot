#!/usr/bin/env bash
#
# Check that a built image carries the config contract's `dev.terrace.config.*` labels.
#
#   verify-contract-labels.sh <labels.json> <contract.labels> <what>
#
#     labels.json       the image's labels as a JSON object, however the caller obtained them
#     contract.labels   `--format labels` from the same generator run that produced the document
#     what              how to name this image in a failure, e.g. "linux/arm64"
#
# This is `Contract::verify_labels` from the other side, and it mirrors it exactly: presence and
# equality of the labels the generator emitted, and nothing more. Extra labels are ignored on
# purpose — every image carries `org.opencontainers.image.*` and whatever its base contributed,
# and none of that is this document's business.
#
# It checks the **image**, never the Dockerfile. A source diff cannot see a base image that
# overrode a label, a `LABEL` line deleted on a branch nobody diffed, or a build argument that
# silently failed to interpolate. This sees what a registry will actually serve.
#
# Two things it refuses rather than passes, because both look like success:
#
#   - a labels document that is not a JSON object. `docker inspect` reports `.Config.Labels` and
#     `crane config` reports `.config.Labels`; reading the wrong one yields `null`, and a
#     careless comparison treats that as "nothing to compare".
#   - an empty expectation file. A generator that wrote nothing would otherwise make the loop
#     below trivially true, which is the one failure this whole scheme cannot afford.
#
# Every violation is reported before exiting. A build that names one missing label and hides two
# is a second round trip.

set -euo pipefail

if [ "$#" -ne 3 ]; then
  echo "usage: $0 <labels.json> <contract.labels> <what>" >&2
  exit 2
fi

labels_json=$1
expected_file=$2
what=$3

if ! command -v jq >/dev/null 2>&1; then
  echo "error: ${what}: jq is not on PATH, so no comparison was made. This is not a passing" >&2
  echo "       image — it is an unrun check." >&2
  exit 1
fi

for file in "${labels_json}" "${expected_file}"; do
  if [ ! -f "${file}" ]; then
    echo "error: ${what}: ${file} does not exist, so nothing was compared." >&2
    exit 1
  fi
done

if ! jq -e 'type == "object"' "${labels_json}" >/dev/null 2>&1; then
  echo "error: ${what}: ${labels_json} holds $(jq -r 'type' "${labels_json}" 2>/dev/null || echo 'unparseable JSON'), not an object." >&2
  echo "       An image with no labels at all reports {}; a null means the wrong JSON path was" >&2
  echo "       read — 'docker inspect' says .Config.Labels and 'crane config' says .config.Labels." >&2
  exit 1
fi

expected_count=$(grep -c '=' "${expected_file}" || true)
if [ "${expected_count}" -eq 0 ]; then
  echo "error: ${what}: ${expected_file} declares no labels, so this check would pass for any" >&2
  echo "       image at all. Regenerate it with '--format labels'." >&2
  exit 1
fi

status=0
while IFS='=' read -r name expected; do
  [ -n "${name}" ] || continue

  actual=$(jq -r --arg n "${name}" '.[$n] // ""' "${labels_json}")
  if [ "${actual}" = "${expected}" ]; then
    continue
  fi

  if [ -z "${actual}" ]; then
    echo "error: ${what}: the image carries no '${name}', so nothing can discover this contract" >&2
    echo "       from its config blob." >&2
  else
    echo "error: ${what}: the image's '${name}' is '${actual}', and this contract's is '${expected}'." >&2
  fi
  status=1
done < "${expected_file}"

if [ "${status}" -eq 0 ]; then
  echo "${what}: ${expected_count} contract labels match the generated document."
else
  echo "       'cargo run --features config-schema --example config-contract -- --format dockerfile'" >&2
  echo "       emits the block the Dockerfile should carry." >&2
fi

exit "${status}"
