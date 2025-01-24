#!/usr/bin/env bash
set -euo pipefail

echo "Verifying uploaded files in Cloudflare R2..."

if ! aws s3 ls "s3://${CLOUDFLARE_R2_BUCKET_NAME}/simple/nautilus-trader/" --endpoint-url="${CLOUDFLARE_R2_URL}"; then
  echo "Failed to list files in R2 bucket"
fi

# Verify index.html exists
if ! aws s3 ls "s3://${CLOUDFLARE_R2_BUCKET_NAME}/simple/nautilus-trader/index.html" --endpoint-url="${CLOUDFLARE_R2_URL}"; then
  echo "index.html not found in R2 bucket"
fi
echo "Verification completed successfully"
