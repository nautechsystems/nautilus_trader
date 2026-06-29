#!/usr/bin/env bash
# Verify that a GitHub release has all final integrity and provenance assets.
set -euo pipefail

expected_draft="${GH_RELEASE_EXPECT_DRAFT:-true}"
verify_attempts="${GH_RELEASE_ASSET_VERIFY_ATTEMPTS:-5}"

if ! [[ "$verify_attempts" =~ ^[0-9]+$ ]] || [[ "$verify_attempts" -lt 1 ]]; then
  echo "::error::GH_RELEASE_ASSET_VERIFY_ATTEMPTS must be a positive integer."
  exit 1
fi
case "$expected_draft" in
  true | false)
    ;;
  *)
    echo "::error::GH_RELEASE_EXPECT_DRAFT must be true or false."
    exit 1
    ;;
esac
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
if ! command -v jq > /dev/null; then
  echo "::error::jq not found."
  exit 1
fi

retry_gh() {
  local description=$1
  shift

  local status=0
  for i in $(seq 1 "$verify_attempts"); do
    "$@" && return 0
    status=$?

    echo "${description} failed (exit=${status}), retry (${i}/${verify_attempts})" >&2
    sleep $((2 ** i))
  done

  echo "::error::${description} failed after retries." >&2
  return "$status"
}

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

release_json="${work_dir}/release.json"
asset_names="${work_dir}/release-asset-names.txt"

retry_gh "gh release view" gh release view "$TAG_NAME" \
  --repo "$GITHUB_REPOSITORY" \
  --json isDraft,assets > "$release_json"

is_draft="$(jq -r '.isDraft' "$release_json")"
if [[ "$is_draft" != "$expected_draft" ]]; then
  echo "::error::Release $TAG_NAME draft state was $is_draft, expected $expected_draft."
  exit 1
fi

jq -r '.assets[].name' "$release_json" | sort > "$asset_names"

duplicate_names="$(jq -r '.assets[].name' "$release_json" | sort | uniq -d)"
if [[ -n "$duplicate_names" ]]; then
  echo "::error::Release $TAG_NAME contains duplicate asset names:"
  printf '%s\n' "$duplicate_names"
  exit 1
fi

invalid_assets="$(jq -r '
  .assets[]
  | select((.state // "uploaded") != "uploaded" or (.size // 0) <= 0)
  | [.name, (.state // "unknown"), (.size // 0)]
  | @tsv
' "$release_json")"
if [[ -n "$invalid_assets" ]]; then
  echo "::error::Release $TAG_NAME contains incomplete assets:"
  printf '%s\n' "$invalid_assets"
  exit 1
fi

require_asset() {
  local asset_name=$1

  if ! grep -Fx -- "$asset_name" "$asset_names" > /dev/null; then
    echo "::error::Release $TAG_NAME is missing asset: $asset_name"
    exit 1
  fi
}

require_asset SHA256SUMS
require_asset dist-manifest.json
require_asset crates-manifest.json

retry_gh "gh release download manifests" gh release download "$TAG_NAME" \
  --repo "$GITHUB_REPOSITORY" \
  --dir "$work_dir" \
  --pattern dist-manifest.json \
  --pattern crates-manifest.json \
  --clobber

if ! jq -e '.artifacts | length > 0' "$work_dir/dist-manifest.json" > /dev/null; then
  echo "::error::dist-manifest.json contains no artifacts."
  exit 1
fi
if ! jq -e '.crates | length > 0' "$work_dir/crates-manifest.json" > /dev/null; then
  echo "::error::crates-manifest.json contains no crates."
  exit 1
fi

jq -r '.artifacts[].name' "$work_dir/dist-manifest.json" |
  while IFS= read -r artifact_name; do
    [[ -z "$artifact_name" ]] && continue

    require_asset "$artifact_name"
    require_asset "${artifact_name}.sha256"
    require_asset "${artifact_name}.sigstore"
    require_asset "${artifact_name}.intoto.jsonl"
  done

echo "Verified final release assets for $TAG_NAME."
