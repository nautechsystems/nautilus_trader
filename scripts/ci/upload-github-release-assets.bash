#!/usr/bin/env bash
# Upload assets to a GitHub release with retries
set -euo pipefail

verify_existing=false
if [[ "${1:-}" == "--verify-existing" ]]; then
  verify_existing=true
  shift
fi
if [[ $# -lt 2 ]]; then
  echo "::error::Usage: upload-github-release-assets.bash [--verify-existing] <label> <asset> [<asset>...]"
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
if [[ "$verify_existing" == "true" ]]; then
  work_dir="$(mktemp -d)"
  trap 'rm -rf "$work_dir"' EXIT
  gh release view "$TAG_NAME" --repo "$GITHUB_REPOSITORY" --json isDraft,assets \
    > "${work_dir}/release.json"
  jq -e '.isDraft == true' "${work_dir}/release.json" > /dev/null || {
    echo "::error::Recovery cannot upload assets to a published release."
    exit 1
  }
fi
for asset in "$@"; do
  if [[ ! -f "$asset" ]]; then
    echo "::error::Release asset not found: $asset"
    exit 1
  fi
  if [[ "$verify_existing" == "true" ]]; then
    name="${asset##*/}"
    count="$(jq --arg name "$name" '[.assets[] | select(.name == $name)] | length' \
      "${work_dir}/release.json")"
    if [[ "$count" -gt 1 ]]; then
      echo "::error::Duplicate release asset: ${name}"
      exit 1
    fi
    if [[ "$count" -eq 1 ]]; then
      gh release download "$TAG_NAME" --repo "$GITHUB_REPOSITORY" \
        --pattern "$name" --dir "$work_dir" --clobber
      if ! cmp -s "$asset" "${work_dir}/${name}"; then
        echo "::error::Existing release asset content mismatch: ${name}"
        exit 1
      fi
      echo "Verified existing release asset ${name}"
      continue
    fi
  fi
  assets+=("$asset")
done
if [[ "${#assets[@]}" -eq 0 ]]; then
  exit 0
fi
clobber=--clobber
if [[ "$verify_existing" == "true" ]]; then
  clobber=
fi

set +e
status=0
for i in $(seq 1 "$upload_attempts"); do
  gh release upload "$TAG_NAME" "${assets[@]}" ${clobber:+"$clobber"} --repo "$GITHUB_REPOSITORY"
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
