#!/usr/bin/env bash
set -euo pipefail

for i in {1..3}; do
  if aws s3 cp index.html "s3://${CLOUDFLARE_R2_BUCKET_NAME}/simple/nautilus-trader/index.html" \
    --endpoint-url="${CLOUDFLARE_R2_URL}" \
    --content-type "text/html; charset=utf-8"; then
    echo "Successfully uploaded index.html"
    break
  else
    echo "Failed to upload index.html, retrying ($i/3)..."
    sleep 5
  fi
done

if [ $i -eq 3 ]; then
  echo "Failed to upload index.html after 3 attempts"
  exit 1
fi
