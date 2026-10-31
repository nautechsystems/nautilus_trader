#!/usr/bin/env bash
set -euo pipefail

repo_root=$(git rev-parse --show-toplevel)
script="$repo_root/scripts/ci/configure-r2-aws.bash"
case_root=$(mktemp -d)
trap 'rm -rf "$case_root"' EXIT
fake_bin="$case_root/bin"
mkdir -p "$fake_bin" "$case_root/home" "$case_root/runner temp"
export RUNNER_TEMP="$case_root/runner temp"
export GITHUB_ENV="$case_root/environment"

printf '%s\n' \
  '#!/usr/bin/env bash' \
  'echo "aws-cli/2.test"' \
  > "$fake_bin/aws"
chmod +x "$fake_bin/aws"

HOME="$case_root/home" \
  PATH="$fake_bin:$PATH" \
  AWS_ACCESS_KEY_ID=test-access \
  AWS_SECRET_ACCESS_KEY=test-secret \
  CLOUDFLARE_R2_REGION=test-region \
  bash "$script" > "$case_root/stdout"

grep -Fxq 'aws-cli/2.test' "$case_root/stdout"
aws_config_file=$(sed -n 's/^AWS_CONFIG_FILE=//p' "$GITHUB_ENV")
[[ "$aws_config_file" == "$RUNNER_TEMP"/r2-aws.*/config ]]
printf 'AWS_CONFIG_FILE=%s\nAWS_SHARED_CREDENTIALS_FILE=/dev/null\n' "$aws_config_file" |
  cmp - "$GITHUB_ENV"
[[ ! -e "$case_root/home/.aws" ]]
[[ ! -e "${aws_config_file%/config}/credentials" ]]
cat > "$case_root/expected-config" << 'CONFIG'
[default]
region=test-region
output=json
s3 =
    signature_version = s3v4
    addressing_style = path
    payload_signing_enabled = false
    use_accelerate_endpoint = false
    use_dualstack_endpoint = false
    use_fips_endpoint = false
CONFIG
cmp "$case_root/expected-config" "$aws_config_file"
python3 - "$aws_config_file" << 'CHECK'
import os
import sys
from pathlib import Path

path = Path(sys.argv[1])
if os.name != "nt":
    assert path.stat().st_mode & 0o777 == 0o600
    assert path.parent.stat().st_mode & 0o777 == 0o700
CHECK
if grep -rFq -e test-access -e test-secret "$case_root"; then
  echo "AWS credentials must not be persisted" >&2
  exit 1
fi

for missing in AWS_ACCESS_KEY_ID AWS_SECRET_ACCESS_KEY; do
  if env HOME="$case_root/missing" PATH="$fake_bin:$PATH" \
    AWS_ACCESS_KEY_ID=test-access AWS_SECRET_ACCESS_KEY=test-secret "$missing=" \
    bash "$script" > /dev/null 2>&1; then
    echo "Expected missing $missing to fail" >&2
    exit 1
  fi
done

echo "R2 AWS configuration tests passed"
