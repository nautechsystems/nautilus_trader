#!/usr/bin/env bash
set -euo pipefail

repo_root=$(git rev-parse --show-toplevel)
script="$repo_root/scripts/ci/configure-r2-aws.bash"
case_root=$(mktemp -d)
trap 'rm -rf "$case_root"' EXIT
fake_bin="$case_root/bin"
mkdir -p "$fake_bin" "$case_root/home"

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
grep -Fxq 'aws_access_key_id=test-access' "$case_root/home/.aws/credentials"
grep -Fxq 'aws_secret_access_key=test-secret' "$case_root/home/.aws/credentials"
grep -Fxq 'region=test-region' "$case_root/home/.aws/config"
grep -Fxq '    addressing_style = path' "$case_root/home/.aws/config"
grep -Fxq '    use_fips_endpoint = false' "$case_root/home/.aws/config"

if HOME="$case_root/missing" PATH="$fake_bin:$PATH" bash "$script" > /dev/null 2>&1; then
  echo "Expected missing AWS credentials to fail" >&2
  exit 1
fi

echo "R2 AWS configuration tests passed"
