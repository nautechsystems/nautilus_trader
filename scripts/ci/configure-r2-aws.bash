#!/usr/bin/env bash
set -euo pipefail

: "${AWS_ACCESS_KEY_ID:?AWS_ACCESS_KEY_ID is required}"
: "${AWS_SECRET_ACCESS_KEY:?AWS_SECRET_ACCESS_KEY is required}"
: "${GITHUB_ENV:?GITHUB_ENV is required}"
: "${RUNNER_TEMP:?RUNNER_TEMP is required}"
region=${CLOUDFLARE_R2_REGION:-auto}

aws --version
umask 077
aws_dir=$(mktemp -d "$RUNNER_TEMP/r2-aws.XXXXXX")

{
  echo "[default]"
  echo "region=${region}"
  echo "output=json"
  echo "s3 ="
  echo "    signature_version = s3v4"
  echo "    addressing_style = path"
  echo "    payload_signing_enabled = false"
  echo "    use_accelerate_endpoint = false"
  echo "    use_dualstack_endpoint = false"
  echo "    use_fips_endpoint = false"
} > "$aws_dir/config"

{
  printf 'AWS_CONFIG_FILE=%s/config\n' "$aws_dir"
  printf 'AWS_SHARED_CREDENTIALS_FILE=/dev/null\n'
} >> "$GITHUB_ENV"
