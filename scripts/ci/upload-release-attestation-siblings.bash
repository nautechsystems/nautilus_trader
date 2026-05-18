#!/usr/bin/env bash
# Upload prepared provenance bundle siblings to a GitHub release
set -euo pipefail

siblings_dir="${1:-release-attestations}"
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"

if [[ ! -d "$siblings_dir" ]]; then
  echo "::error::Attestation sibling directory not found: ${siblings_dir}."
  exit 1
fi

siblings=()
while IFS= read -r -d '' sibling; do
  siblings+=("$sibling")
done < <(
  find "$siblings_dir" -type f \
    \( -name '*.sigstore' -o -name '*.intoto.jsonl' \) \
    -print0 |
    sort -z
)

if [[ "${#siblings[@]}" -eq 0 ]]; then
  echo "::error::No attestation siblings found in ${siblings_dir}."
  exit 1
fi

bash "${script_dir}/upload-github-release-assets.bash" attestation-siblings "${siblings[@]}"
