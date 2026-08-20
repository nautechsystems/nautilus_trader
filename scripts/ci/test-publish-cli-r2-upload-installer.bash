#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "${script_dir}/../.." && pwd)"
upload_script="${script_dir}/publish-cli-r2-upload-installer.sh"
work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

fail() {
  echo "::error::$1" >&2
  exit 1
}

mock_bin="${work_dir}/bin"
mkdir -p "$mock_bin"

cat > "${mock_bin}/aws" << 'MOCK'
#!/usr/bin/env bash
set -euo pipefail

printf '%s\n' "$*" >> "${MOCK_AWS_LOG:?}"
call_count="$(awk 'END { print NR }' "${MOCK_AWS_LOG:?}")"

case ",${MOCK_AWS_SUCCESS_CALLS:-}," in
  *",${call_count},"*) exit 0 ;;
esac

exit "${MOCK_AWS_FAILURE_STATUS:?}"
MOCK

cat > "${mock_bin}/sleep" << 'MOCK'
#!/usr/bin/env bash
set -euo pipefail

printf '%s\n' "$1" >> "${MOCK_SLEEP_LOG:?}"
MOCK

chmod +x "${mock_bin}/aws" "${mock_bin}/sleep"

failure_status=23
max_attempts=5
bucket="test-bucket"
prefix="cli/test"
r2_url="https://test.r2.cloudflarestorage.com"
installer="scripts/cli/install.sh"
stable_destination="s3://${bucket}/${prefix}/install.sh"
latest_destination="s3://${bucket}/${prefix}/latest/install.sh"

count_occurrences() {
  local file=$1
  local text=$2

  awk -v text="$text" 'index($0, text) { count++ } END { print count + 0 }' "$file"
}

assert_occurrences() {
  local file=$1
  local text=$2
  local expected=$3
  local actual

  actual="$(count_occurrences "$file" "$text")"
  if [[ "$actual" != "$expected" ]]; then
    fail "Expected ${expected} occurrences of '${text}', found ${actual}."
  fi
}

run_exhaustion_case() {
  local name=$1
  local success_calls=$2
  local expected_calls=$3
  local expected_stable=$4
  local expected_latest=$5
  local case_dir="${work_dir}/${name}"
  local aws_log="${case_dir}/aws.log"
  local sleep_log="${case_dir}/sleep.log"
  local output="${case_dir}/output.log"
  local expected_sleep_log="${case_dir}/expected-sleep.log"
  local status

  mkdir -p "$case_dir"
  : > "$aws_log"
  : > "$sleep_log"
  printf '2\n4\n8\n16\n' > "$expected_sleep_log"

  set +e
  (
    cd "$repo_root"
    PATH="${mock_bin}:$PATH" \
      MOCK_AWS_FAILURE_STATUS="$failure_status" \
      MOCK_AWS_LOG="$aws_log" \
      MOCK_AWS_SUCCESS_CALLS="$success_calls" \
      MOCK_SLEEP_LOG="$sleep_log" \
      CLOUDFLARE_R2_BUCKET_NAME="$bucket" \
      CLOUDFLARE_R2_PREFIX="$prefix" \
      CLOUDFLARE_R2_URL="$r2_url" \
      bash "$upload_script"
  ) > "$output" 2>&1
  status=$?
  set -e

  if [[ "$status" -ne "$failure_status" ]]; then
    fail "${name} should return the last aws failure status ${failure_status}, found ${status}."
  fi

  assert_occurrences "$aws_log" "s3 cp ${installer}" "$expected_calls"
  assert_occurrences "$aws_log" "$stable_destination" "$expected_stable"
  assert_occurrences "$aws_log" "$latest_destination" "$expected_latest"
  assert_occurrences "$aws_log" "--endpoint-url=${r2_url}" "$expected_calls"
  assert_occurrences "$aws_log" "--content-type text/x-shellscript" "$expected_calls"
  assert_occurrences "$aws_log" "--cache-control no-cache, max-age=60, must-revalidate" "$expected_calls"

  if ! cmp -s "$expected_sleep_log" "$sleep_log"; then
    fail "${name} should sleep only between failed attempts."
  fi
  if ! grep -Fq "Failed to upload ${installer} after ${max_attempts} attempts" "$output"; then
    fail "${name} should report retry exhaustion."
  fi
}

run_success_case() {
  local case_dir="${work_dir}/transient-success"
  local aws_log="${case_dir}/aws.log"
  local sleep_log="${case_dir}/sleep.log"
  local output="${case_dir}/output.log"
  local expected_sleep_log="${case_dir}/expected-sleep.log"

  mkdir -p "$case_dir"
  : > "$aws_log"
  : > "$sleep_log"
  printf '2\n4\n2\n' > "$expected_sleep_log"

  (
    cd "$repo_root"
    PATH="${mock_bin}:$PATH" \
      MOCK_AWS_FAILURE_STATUS="$failure_status" \
      MOCK_AWS_LOG="$aws_log" \
      MOCK_AWS_SUCCESS_CALLS="3,5" \
      MOCK_SLEEP_LOG="$sleep_log" \
      CLOUDFLARE_R2_BUCKET_NAME="$bucket" \
      CLOUDFLARE_R2_PREFIX="$prefix" \
      CLOUDFLARE_R2_URL="$r2_url" \
      bash "$upload_script"
  ) > "$output" 2>&1

  assert_occurrences "$aws_log" "s3 cp ${installer}" 5
  assert_occurrences "$aws_log" "$stable_destination" 3
  assert_occurrences "$aws_log" "$latest_destination" 2
  assert_occurrences "$output" "Upload failed (exit=${failure_status})" 3

  if ! cmp -s "$expected_sleep_log" "$sleep_log"; then
    fail "transient-success should reset retry delays for the latest destination."
  fi
  if grep -Fq "Failed to upload" "$output"; then
    fail "transient-success should not report retry exhaustion."
  fi
}

run_exhaustion_case "stable-exhaustion" "" "$max_attempts" "$max_attempts" 0
run_exhaustion_case "latest-exhaustion" "1" "$((max_attempts + 1))" 1 "$max_attempts"
run_success_case

echo "CLI R2 installer upload retry tests passed"
