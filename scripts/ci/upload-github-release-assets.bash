#!/usr/bin/env bash
# Upload assets to a GitHub release with retries
set -euo pipefail

if [[ $# -lt 2 ]]; then
  echo "::error::Usage: upload-github-release-assets.bash <label> <asset> [<asset>...]"
  exit 1
fi

asset_label=$1
shift
upload_attempts="${GH_RELEASE_UPLOAD_ATTEMPTS:-5}"

if ! [[ "$upload_attempts" =~ ^[0-9]+$ ]] || [[ "$upload_attempts" -lt 1 ]]; then
  echo "::error::GH_RELEASE_UPLOAD_ATTEMPTS must be a positive integer."
  exit 1
fi
if [[ -z "${TAG_NAME:-}" ]]; then
  echo "::error::TAG_NAME not set."
  exit 1
fi
if [[ -z "${GITHUB_REPOSITORY:-}" ]]; then
  echo "::error::GITHUB_REPOSITORY not set."
  exit 1
fi
if [[ -z "${GITHUB_TOKEN:-}" && -z "${GH_TOKEN:-}" ]]; then
  echo "::error::GITHUB_TOKEN or GH_TOKEN not set."
  exit 1
fi
if ! command -v gh > /dev/null; then
  echo "::error::gh not found."
  exit 1
fi

assets=()
for asset in "$@"; do
  if [[ ! -f "$asset" ]]; then
    echo "::error::Release asset not found: $asset"
    exit 1
  fi
  assets+=("$asset")
done

set +e
status=0
for i in $(seq 1 "$upload_attempts"); do
  gh release upload "$TAG_NAME" "${assets[@]}" --clobber --repo "$GITHUB_REPOSITORY"
  status=$?
  if [[ "$status" -eq 0 ]]; then
    echo "Uploaded ${#assets[@]} ${asset_label} asset(s) to release $TAG_NAME."
    exit 0
  fi

  echo "gh upload (${asset_label}) failed (exit=$status), retry ($i/${upload_attempts})"
  sleep $((2 ** i))
done
set -e

echo "::error::Failed to upload ${asset_label} asset(s) after retries."
exit "$status"
