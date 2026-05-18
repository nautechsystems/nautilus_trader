#!/usr/bin/env bash
# Publish files in dist/ to PyPI using Trusted Publishing with retries
set -euo pipefail

artifact_label="${1:-distributions}"
pypi_publish_attempts="${PYPI_PUBLISH_ATTEMPTS:-5}"
pypi_publish_check_url="${PYPI_PUBLISH_CHECK_URL:-https://pypi.org/simple/}"

if ! [[ "$pypi_publish_attempts" =~ ^[0-9]+$ ]] || [[ "$pypi_publish_attempts" -lt 1 ]]; then
  echo "::error::PYPI_PUBLISH_ATTEMPTS must be a positive integer."
  exit 1
fi
if ! command -v uv > /dev/null; then
  echo "::error::uv not found."
  exit 1
fi

set +e
success=false
status=0
for i in $(seq 1 "$pypi_publish_attempts"); do
  uv publish --trusted-publishing automatic --check-url "$pypi_publish_check_url"
  status=$?
  if [[ "$status" -eq 0 ]]; then
    success=true
    break
  fi

  echo "uv publish failed (exit=$status), retry ($i/${pypi_publish_attempts})"
  sleep $((2 ** i))
done
set -e

if [[ "$success" = false ]]; then
  echo "::error::Failed to publish ${artifact_label} to PyPI after retries."
  exit "$status"
fi
