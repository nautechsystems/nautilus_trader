#!/usr/bin/env bash
set -euo pipefail

echo "Uploading new wheels to Cloudflare R2..."

echo "dist/ contents:"
ls -la dist/
find dist/ -type f -name "nautilus_trader-*.whl" -ls

# Verify wheels exist
if ! find dist/ -type f -name "nautilus_trader-*.whl" -print -quit | grep -q .; then
  echo "ERROR: No wheels found in dist/"
  exit 1
fi

wheel_count=0
for file in dist/nautilus_trader-*.whl; do
  echo "File details for $file:"
  ls -l "$file"
  file "$file"

  if [ ! -f "$file" ]; then
    echo "Warning: '$file' is not a regular file, skipping"
    continue
  fi

  wheel_count=$((wheel_count + 1))
  echo "Found wheel: $file"
  echo "sha256:$(sha256sum "$file" | awk '{print $1}')"

  echo "Uploading $file..."
  set +e
  success=false
  for i in {1..5}; do
    aws s3 cp "$file" "s3://${CLOUDFLARE_R2_BUCKET_NAME}/${CLOUDFLARE_R2_PREFIX:-simple/nautilus-trader}/" \
      --endpoint-url="${CLOUDFLARE_R2_URL}" \
      --content-type "application/zip"
    status=$?
    if [ $status -eq 0 ]; then
      echo "Successfully uploaded $file"
      success=true
      break
    else
      echo "Upload failed for $file (exit=$status), retrying ($i/5)..."
      sleep $((2 ** i))
    fi
  done
  set -e
  if [ "$success" = false ]; then
    echo "Failed to upload $file after 5 attempts"
    exit 1
  fi
done

if [ "$wheel_count" -eq 0 ]; then
  echo "No wheel files found in dist directory"
  exit 1
fi

echo "Successfully uploaded $wheel_count wheel files"
