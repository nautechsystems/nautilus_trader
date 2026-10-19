#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
index_file="${PUBLISH_WHEELS_INDEX_FILE:?PUBLISH_WHEELS_INDEX_FILE is required}"
: "${PUBLISH_WHEELS_MANIFEST:?PUBLISH_WHEELS_MANIFEST is required}"
: "${PUBLISH_WHEELS_DELETE_MANIFEST:?PUBLISH_WHEELS_DELETE_MANIFEST is required}"
bucket="${CLOUDFLARE_R2_BUCKET_NAME:?CLOUDFLARE_R2_BUCKET_NAME is required}"
prefix="${CLOUDFLARE_R2_PREFIX:?CLOUDFLARE_R2_PREFIX is required}"
endpoint="${CLOUDFLARE_R2_URL:?CLOUDFLARE_R2_URL is required}"
etag_file="${index_file}.etag"

success=false
attempt=1
while [[ "$attempt" -le 5 ]]; do
  etag="$(cat "$etag_file")"
  if [[ -z "$etag" || "$etag" == "None" ]]; then
    echo "Error: Generated index is missing its write precondition" >&2
    exit 1
  fi
  condition=(--if-match "$etag")
  if [[ "$etag" == "*" ]]; then
    condition=(--if-none-match '*')
  fi

  if aws s3api put-object --bucket "$bucket" --key "${prefix}/index.html" \
    --body "$index_file" "${condition[@]}" \
    --endpoint-url="$endpoint" \
    --content-type "text/html; charset=utf-8" \
    --cache-control "no-cache, max-age=60, must-revalidate" \
    --cli-connect-timeout 10 --cli-read-timeout 60; then
    success=true
    break
  fi

  if [[ "$attempt" -lt 5 ]]; then
    echo "Index upload failed, rebuilding from R2 before retrying (${attempt}/5)"
    sleep $((2 ** attempt))
    bash "${script_dir}/publish-wheels-generate-index.sh"
  fi
  attempt=$((attempt + 1))
done

if [[ "$success" != "true" ]]; then
  echo "Error: Failed to upload index after 5 attempts" >&2
  exit 1
fi

echo "Uploaded index.html"
