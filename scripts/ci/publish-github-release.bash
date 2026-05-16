#!/usr/bin/env bash
# Publish a draft GitHub release after all immutable assets are attached
set -euo pipefail

publish_attempts="${GH_RELEASE_PUBLISH_ATTEMPTS:-5}"

if ! [[ "$publish_attempts" =~ ^[0-9]+$ ]] || [[ "$publish_attempts" -lt 1 ]]; then
  echo "::error::GH_RELEASE_PUBLISH_ATTEMPTS must be a positive integer."
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

retry_gh() {
  local description=$1
  shift

  local status=0
  for i in $(seq 1 "$publish_attempts"); do
    if "$@"; then
      return 0
    fi
    status=$?

    echo "${description} failed (exit=${status}), retry (${i}/${publish_attempts})" >&2
    sleep $((2 ** i))
  done

  echo "::error::${description} failed after retries." >&2
  return "$status"
}

release_info="$(
  retry_gh "gh release view" gh release view "$TAG_NAME" \
    --repo "$GITHUB_REPOSITORY" \
    --json isDraft,url \
    --jq '[.isDraft, .url] | @tsv'
)"
is_draft="${release_info%%$'\t'*}"
release_url="${release_info#*$'\t'}"

if [[ "$is_draft" != "true" ]]; then
  echo "Release $TAG_NAME is already published: $release_url"
  exit 0
fi

if retry_gh "gh release publish" gh release edit "$TAG_NAME" --repo "$GITHUB_REPOSITORY" --draft=false; then
  echo "Published GitHub release $TAG_NAME: $release_url"
  exit 0
else
  status=$?
  exit "$status"
fi
