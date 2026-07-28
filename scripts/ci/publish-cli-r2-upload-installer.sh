#!/usr/bin/env bash
set -euo pipefail

PREFIX=${CLOUDFLARE_R2_PREFIX:-cli/nautilus-cli}
BUCKET=${CLOUDFLARE_R2_BUCKET_NAME:?CLOUDFLARE_R2_BUCKET_NAME not set}
R2_URL=${CLOUDFLARE_R2_URL:?CLOUDFLARE_R2_URL not set}

INSTALLER="scripts/cli/install.sh"
CACHE_CONTROL="no-cache, max-age=60, must-revalidate"
MAX_ATTEMPTS=5

upload_with_retry() {
  local destination=$1
  local attempt
  local status=0

  for ((attempt = 1; attempt <= MAX_ATTEMPTS; attempt++)); do
    if aws s3 cp "$INSTALLER" "$destination" \
      --endpoint-url="$R2_URL" \
      --content-type "text/x-shellscript" \
      --cache-control "$CACHE_CONTROL"; then
      return 0
    else
      status=$?
    fi

    if [ "$attempt" -lt "$MAX_ATTEMPTS" ]; then
      echo "Upload failed (exit=$status), retry ($attempt/$MAX_ATTEMPTS)"
      sleep "$((2 ** attempt))"
    fi
  done

  echo "Failed to upload $INSTALLER after $MAX_ATTEMPTS attempts: $destination" >&2
  return "$status"
}

upload_with_retry "s3://${BUCKET}/${PREFIX}/install.sh"
upload_with_retry "s3://${BUCKET}/${PREFIX}/latest/install.sh"
