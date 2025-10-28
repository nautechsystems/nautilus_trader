#!/usr/bin/env bash
set -euo pipefail

echo "Verifying uploaded files in Cloudflare R2..."

ok=false
for i in {1..5}; do
  if aws s3 ls "s3://${CLOUDFLARE_R2_BUCKET_NAME}/${CLOUDFLARE_R2_PREFIX:-simple/nautilus-trader}/" \
    --endpoint-url="${CLOUDFLARE_R2_URL}" --cli-connect-timeout 10 --cli-read-timeout 60; then
    ok=true
    break
  else
    echo "Failed to list files in R2 bucket, retrying ($i/5)..."
    sleep $((2 ** i))
  fi
done
if [ "$ok" = false ]; then
  echo "Error: Could not list files in R2 bucket after retries"
  exit 1
fi

# Verify index.html exists
ok_index=false
for i in {1..5}; do
  if aws s3 ls "s3://${CLOUDFLARE_R2_BUCKET_NAME}/${CLOUDFLARE_R2_PREFIX:-simple/nautilus-trader}/index.html" \
    --endpoint-url="${CLOUDFLARE_R2_URL}" --cli-connect-timeout 10 --cli-read-timeout 60; then
    ok_index=true
    break
  else
    echo "index.html not found yet, retrying ($i/5)..."
    sleep $((2 ** i))
  fi
done
if [ "$ok_index" = false ]; then
  echo "Error: index.html not found in R2 bucket after retries"
  exit 1
fi
echo "Verification completed"
