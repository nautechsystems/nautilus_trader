#!/usr/bin/env bash
# Attach Sigstore bundle siblings to a GitHub release so Scorecard's
# Signed-Releases check sees coverage. The bundle from
# actions/attest-build-provenance is copied next to each release asset as
# <asset>.sigstore, then uploaded to the release.
#
# Usage:
#   publish-release-attestation-siblings.sh <asset> [<asset>...]
#
# Required env:
#   TAG_NAME          - release tag to upload to
#   BUNDLE_PATH       - path to the Sigstore bundle (steps.<id>.outputs.bundle-path)
#   GITHUB_TOKEN      - for gh CLI auth
#   GITHUB_REPOSITORY - owner/repo (set by GitHub Actions)
#
# Exit codes:
#   0 - siblings attached, or skipped with a warning (no inputs)
#   1 - bundle present but upload failed after retries
set -uo pipefail

if [[ -z "${TAG_NAME:-}" ]]; then
  echo "::error::TAG_NAME not set."
  exit 1
fi
if [[ -z "${GITHUB_REPOSITORY:-}" ]]; then
  echo "::error::GITHUB_REPOSITORY not set."
  exit 1
fi
if [[ -z "${BUNDLE_PATH:-}" || ! -f "$BUNDLE_PATH" ]]; then
  echo "::warning::Attestation bundle missing (path='${BUNDLE_PATH:-}'); skipping siblings."
  exit 0
fi
if [[ $# -eq 0 ]]; then
  echo "::warning::No assets passed; nothing to attach."
  exit 0
fi

siblings=()
for asset in "$@"; do
  if [[ ! -f "$asset" ]]; then
    echo "::warning::Asset not found, skipping: $asset"
    continue
  fi
  sib="${asset}.sigstore"
  cp "$BUNDLE_PATH" "$sib"
  siblings+=("$sib")
done

if [[ ${#siblings[@]} -eq 0 ]]; then
  echo "::warning::No valid assets resolved; nothing to attach."
  exit 0
fi

echo "Prepared ${#siblings[@]} attestation sibling(s):"
printf '  %s\n' "${siblings[@]}"

set +e
for i in {1..5}; do
  gh release upload "$TAG_NAME" "${siblings[@]}" --clobber --repo "$GITHUB_REPOSITORY"
  status=$?
  if [ $status -eq 0 ]; then
    echo "Attached ${#siblings[@]} attestation sibling(s) to release $TAG_NAME."
    exit 0
  fi
  echo "gh upload (siblings) failed (exit=$status), retry ($i/5)"
  sleep $((2 ** i))
done

echo "::error::Failed to upload attestation siblings after retries (non-blocking)."
exit 1
