#!/usr/bin/env bash
set -euo pipefail

echo "Uploading new wheels to Cloudflare R2..."

echo "Initial dist/ contents:"
ls -la dist/
find dist/ -type f -name "*.whl" -ls

# Create clean directory for real files
mkdir -p dist/all

# Copy all files into dist/all/ to resolve symlinks
find dist/ -type f -name "*.whl" -exec cp -L {} dist/all/ \;

# First check for any wheels
if ! find dist/all/ -type f -name "*.whl" >/dev/null 2>&1; then
  echo "No wheels found in dist/all/, exiting"
  exit 1
fi

echo "Contents of dist/all/:"
ls -la dist/all/

wheel_count=0
for file in dist/all/*.whl; do
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
  for i in {1..3}; do
    if aws s3 cp "$file" "s3://${CLOUDFLARE_R2_BUCKET_NAME}/simple/nautilus-trader/" \
      --endpoint-url="${CLOUDFLARE_R2_URL}" \
      --content-type "application/zip"; then
      echo "Successfully uploaded $file"
      break
    else
      echo "Upload failed for $file, retrying ($i/3)..."
      sleep 5
    fi

    if [ $i -eq 3 ]; then
      echo "Failed to upload $file after 3 attempts"
    fi
  done
done

if [ "$wheel_count" -eq 0 ]; then
  echo "No wheel files found in dist directory"
  exit 1
fi

echo "Successfully uploaded $wheel_count wheel files"
