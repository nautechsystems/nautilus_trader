#!/usr/bin/env bash
set -euo pipefail

index_file="${PUBLISH_WHEELS_INDEX_FILE:?PUBLISH_WHEELS_INDEX_FILE is required}"
bucket="${CLOUDFLARE_R2_BUCKET_NAME:?CLOUDFLARE_R2_BUCKET_NAME is required}"
prefix="${CLOUDFLARE_R2_PREFIX:?CLOUDFLARE_R2_PREFIX is required}"
endpoint="${CLOUDFLARE_R2_URL:?CLOUDFLARE_R2_URL is required}"
destination="s3://${bucket}/${prefix}/index.html"

success=false
attempt=1
while [[ "$attempt" -le 5 ]]; do
  if aws s3 cp "$index_file" "$destination" \
    --endpoint-url="$endpoint" \
    --content-type "text/html; charset=utf-8" \
    --cache-control "no-cache, max-age=60, must-revalidate" \
    --cli-connect-timeout 10 --cli-read-timeout 60 \
    --no-progress; then
    success=true
    break
  fi

  if [[ "$attempt" -lt 5 ]]; then
    echo "Index upload failed, retrying (${attempt}/5)"
    sleep $((2 ** attempt))
  fi
  attempt=$((attempt + 1))
done

if [[ "$success" != "true" ]]; then
  echo "Error: Failed to upload index after 5 attempts" >&2
  exit 1
fi

echo "Uploaded index.html"
