#!/usr/bin/env bash
set -euo pipefail

aws_access_key=${AWS_ACCESS_KEY_ID:?AWS_ACCESS_KEY_ID is required}
aws_secret_key=${AWS_SECRET_ACCESS_KEY:?AWS_SECRET_ACCESS_KEY is required}
region=${CLOUDFLARE_R2_REGION:-auto}
aws_dir=${HOME:?HOME is required}/.aws

aws --version
mkdir -p "$aws_dir"
{
  echo "[default]"
  echo "aws_access_key_id=${aws_access_key}"
  echo "aws_secret_access_key=${aws_secret_key}"
} > "$aws_dir/credentials"

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
