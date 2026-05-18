#!/usr/bin/env bash
# Verify the immutable GitHub release attestation after publication
set -euo pipefail

verify_attempts="${GH_RELEASE_VERIFY_ATTEMPTS:-5}"

if ! [[ "$verify_attempts" =~ ^[0-9]+$ ]] || [[ "$verify_attempts" -lt 1 ]]; then
  echo "::error::GH_RELEASE_VERIFY_ATTEMPTS must be a positive integer."
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

if ! gh release verify --help > /dev/null 2>&1; then
  echo "::error::This GitHub CLI version does not support 'gh release verify'."
  exit 1
fi

status=0
for i in $(seq 1 "$verify_attempts"); do
  if gh release verify "$TAG_NAME" --repo "$GITHUB_REPOSITORY"; then
    echo "Verified immutable GitHub release attestation for $TAG_NAME."
    exit 0
  fi
  status=$?

  echo "gh release verify failed (exit=${status}), retry (${i}/${verify_attempts})" >&2
  sleep $((2 ** i))
done

echo "::error::gh release verify failed after retries." >&2
exit "$status"
