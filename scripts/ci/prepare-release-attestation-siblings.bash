#!/usr/bin/env bash
# Copy a provenance bundle next to release assets for later release upload
set -euo pipefail

if [[ -z "${BUNDLE_PATH:-}" || ! -f "$BUNDLE_PATH" ]]; then
  echo "::error::Attestation bundle missing: ${BUNDLE_PATH:-<unset>}"
  exit 1
fi
if [[ $# -eq 0 ]]; then
  echo "::error::No release assets passed."
  exit 1
fi
if ! command -v jq > /dev/null; then
  echo "::error::jq not found."
  exit 1
fi

siblings=()
for asset in "$@"; do
  if [[ ! -f "$asset" ]]; then
    echo "::error::Release asset not found: $asset"
    exit 1
  fi

  sigstore_sibling="${asset}.sigstore"
  intoto_sibling="${asset}.intoto.jsonl"
  cp "$BUNDLE_PATH" "$sigstore_sibling"
  jq -c '.dsseEnvelope // empty' "$BUNDLE_PATH" > "$intoto_sibling"
  if [[ ! -s "$intoto_sibling" ]]; then
    echo "::error::Attestation bundle missing dsseEnvelope: $BUNDLE_PATH"
    exit 1
  fi
  siblings+=("$sigstore_sibling" "$intoto_sibling")
done

echo "Prepared ${#siblings[@]} attestation sibling(s):"
printf '  %s\n' "${siblings[@]}"
