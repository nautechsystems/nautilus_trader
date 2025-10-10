#!/usr/bin/env bash
set -euo pipefail

success=false
for i in {1..5}; do
  if aws s3 cp index.html "s3://${CLOUDFLARE_R2_BUCKET_NAME}/${CLOUDFLARE_R2_PREFIX:-simple/nautilus-trader}/index.html" \
    --endpoint-url="${CLOUDFLARE_R2_URL}" \
    --content-type "text/html; charset=utf-8" \
    --cache-control "no-cache, max-age=60, must-revalidate" \
    --cli-connect-timeout 10 --cli-read-timeout 60; then
    echo "Successfully uploaded index.html"
    success=true
    break
  else
    echo "Failed to upload index.html, retrying ($i/5)..."
    sleep $((2 ** i))
  fi
done

if [ "$success" = false ]; then
  echo "Failed to upload index.html after 5 attempts"
  exit 1
fi
