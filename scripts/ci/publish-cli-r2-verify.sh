#!/usr/bin/env bash
set -euo pipefail

PREFIX=${CLOUDFLARE_R2_PREFIX:-cli/nautilus-cli}
BUCKET=${CLOUDFLARE_R2_BUCKET_NAME:?}
R2_URL=${CLOUDFLARE_R2_URL:?}

echo "Verifying contents at s3://${BUCKET}/${PREFIX}/latest/"
aws s3 ls "s3://${BUCKET}/${PREFIX}/latest/" --endpoint-url="$R2_URL" || true

echo "Checking installer presence (stable + latest)"
aws s3 ls "s3://${BUCKET}/${PREFIX}/install.sh" --endpoint-url="$R2_URL" || echo "install.sh (stable) not found"
aws s3 ls "s3://${BUCKET}/${PREFIX}/latest/install.sh" --endpoint-url="$R2_URL" || echo "install.sh (latest) not found"

echo "Done"
