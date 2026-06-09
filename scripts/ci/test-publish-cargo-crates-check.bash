#!/usr/bin/env bash
# Test Cargo publish planning with mocked cargo metadata and crates.io lookups.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "${script_dir}/../.." && pwd)"
work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

fail() {
  echo "::error::$1" >&2
  exit 1
}

write_mock_commands() {
  local mock_bin=$1

  mkdir -p "$mock_bin"

  cat > "${mock_bin}/cargo" << 'MOCK'
#!/usr/bin/env bash
set -euo pipefail

if [[ "$*" == "metadata --no-deps --format-version=1" ]]; then
  cat "${MOCK_CARGO_METADATA:?}"
  exit 0
fi

echo "unexpected cargo args: $*" >&2
exit 2
MOCK

  cat > "${mock_bin}/curl" << 'MOCK'
#!/usr/bin/env bash
set -euo pipefail

output=""
url=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --output)
      output=$2
      shift 2
      ;;
    --proto | --retry | --connect-timeout | --max-time | --header | --write-out)
      shift 2
      ;;
    --tlsv1.2 | --silent | --show-error | --location | --retry-all-errors)
      shift
      ;;
    *)
      url=$1
      shift
      ;;
  esac
done

if [[ -z "$output" || -z "$url" ]]; then
  echo "curl mock missing output or URL" >&2
  exit 2
fi

printf '%s\n' "$url" >> "${MOCK_CURL_LOG:?}"

case "$url" in
  https://crates.io/api/v1/crates/*/*)
    crate_spec=${url#https://crates.io/api/v1/crates/}
    crate_name=${crate_spec%/*}
    crate_version=${crate_spec##*/}
    error_status="$(
      awk -v name="$crate_name" -v version="$crate_version" \
        '$1 == name && $2 == version { print $3; exit }' \
        "${MOCK_CRATES_IO_ERROR_FILE:?}"
    )"
    if [[ -n "$error_status" ]]; then
      printf '{"errors":[{"detail":"mock registry failure"}]}\n' > "$output"
      printf '%s' "$error_status"
      exit 0
    fi
    if grep -Fxq "${crate_name} ${crate_version}" "${MOCK_CRATES_IO_EXISTS_FILE:?}"; then
      printf '{"version":{}}\n' > "$output"
      printf '200'
    else
      printf '{"errors":[{"detail":"Not Found"}]}\n' > "$output"
      printf '404'
    fi
    ;;
  *)
    echo "unexpected curl URL: $url" >&2
    exit 2
    ;;
esac
MOCK

  cat > "${mock_bin}/sleep" << 'MOCK'
#!/usr/bin/env bash
set -euo pipefail

echo "unexpected sleep: $*" >&2
exit 2
MOCK

  chmod +x "${mock_bin}/cargo" "${mock_bin}/curl" "${mock_bin}/sleep"
}

write_dev_build_metadata() {
  local metadata_file=$1

  cat > "$metadata_file" << 'JSON'
{
  "packages": [
    {
      "name": "mock-root",
      "version": "1.0.0",
      "source": null,
      "publish": null,
      "dependencies": [
        {
          "name": "mock-dev",
          "req": "^1.0.0",
          "kind": "dev",
          "optional": false,
          "path": "crates/mock-dev",
          "source": null
        },
        {
          "name": "mock-build",
          "req": "^1.0.0",
          "kind": "build",
          "optional": false,
          "path": "crates/mock-build",
          "source": null
        },
        {
          "name": "serde",
          "req": "^1",
          "kind": null,
          "optional": false,
          "path": null,
          "source": "registry+https://github.com/rust-lang/crates.io-index"
        }
      ]
    },
    {
      "name": "mock-dev",
      "version": "1.0.0",
      "source": null,
      "publish": null,
      "dependencies": []
    },
    {
      "name": "mock-build",
      "version": "1.0.0",
      "source": null,
      "publish": null,
      "dependencies": []
    }
  ]
}
JSON
}

write_git_only_metadata() {
  local metadata_file=$1

  cat > "$metadata_file" << 'JSON'
{
  "packages": [
    {
      "name": "mock-git-parent",
      "version": "1.0.0",
      "source": null,
      "publish": null,
      "dependencies": [
        {
          "name": "ecgfp5",
          "req": "*",
          "kind": "dev",
          "optional": false,
          "path": null,
          "source": "git+https://github.com/example/ecgfp5?rev=abc123#abc123"
        }
      ]
    }
  ]
}
JSON
}

write_cycle_metadata() {
  local metadata_file=$1

  cat > "$metadata_file" << 'JSON'
{
  "packages": [
    {
      "name": "mock-cycle-a",
      "version": "1.0.0",
      "source": null,
      "publish": null,
      "dependencies": [
        {
          "name": "mock-cycle-b",
          "req": "^1.0.0",
          "kind": "dev",
          "optional": false,
          "path": "crates/mock-cycle-b",
          "source": null
        }
      ]
    },
    {
      "name": "mock-cycle-b",
      "version": "1.0.0",
      "source": null,
      "publish": null,
      "dependencies": [
        {
          "name": "mock-cycle-a",
          "req": "^1.0.0",
          "kind": "build",
          "optional": false,
          "path": "crates/mock-cycle-a",
          "source": null
        }
      ]
    }
  ]
}
JSON
}

write_empty_plan_metadata() {
  local metadata_file=$1

  cat > "$metadata_file" << 'JSON'
{
  "packages": [
    {
      "name": "mock-private-only",
      "version": "1.0.0",
      "source": null,
      "publish": [],
      "dependencies": []
    }
  ]
}
JSON
}

write_crates_io_dependency_metadata() {
  local metadata_file=$1

  cat > "$metadata_file" << 'JSON'
{
  "packages": [
    {
      "name": "mock-public",
      "version": "1.0.0",
      "source": null,
      "publish": null,
      "dependencies": [
        {
          "name": "mock-existing-private",
          "req": "^0.3.0",
          "kind": null,
          "optional": false,
          "path": "crates/mock-existing-private",
          "source": null
        }
      ]
    },
    {
      "name": "mock-existing-private",
      "version": "0.3.0",
      "source": null,
      "publish": [],
      "dependencies": []
    }
  ]
}
JSON
}

run_publish_check() {
  local metadata_file=$1
  local exists_file=$2
  local curl_log=$3

  (
    cd "$repo_root"
    env \
      PATH="${mock_bin}:${PATH}" \
      MOCK_CARGO_METADATA="$metadata_file" \
      MOCK_CRATES_IO_ERROR_FILE="$error_file" \
      MOCK_CRATES_IO_EXISTS_FILE="$exists_file" \
      MOCK_CURL_LOG="$curl_log" \
      CURL_CONNECT_TIMEOUT=1 \
      CURL_MAX_TIME=1 \
      CURL_RETRIES=1 \
      bash "${script_dir}/publish-cargo-crates.sh" --check
  )
}

assert_contains() {
  local file=$1
  local pattern=$2
  local message=$3

  if ! grep -Fq "$pattern" "$file"; then
    echo "expected pattern: $pattern" >&2
    echo "actual output:" >&2
    cat "$file" >&2
    fail "$message"
  fi
}

assert_not_contains() {
  local file=$1
  local pattern=$2
  local message=$3

  if grep -Fq "$pattern" "$file"; then
    echo "unexpected pattern: $pattern" >&2
    echo "actual output:" >&2
    cat "$file" >&2
    fail "$message"
  fi
}

mock_bin="${work_dir}/mock-bin"
write_mock_commands "$mock_bin"

exists_file="${work_dir}/existing-crates.txt"
: > "$exists_file"
error_file="${work_dir}/crates-io-errors.txt"
: > "$error_file"

dev_build_metadata="${work_dir}/dev-build-metadata.json"
write_dev_build_metadata "$dev_build_metadata"
dev_build_output="${work_dir}/dev-build-output.txt"
dev_build_curl_log="${work_dir}/dev-build-curl.log"
: > "$dev_build_curl_log"
run_publish_check "$dev_build_metadata" "$exists_file" "$dev_build_curl_log" \
  > "$dev_build_output" 2>&1

assert_contains "$dev_build_output" $'1. mock-dev\t1.0.0' \
  "dev path dependency was not published before its dependent crate."
assert_contains "$dev_build_output" $'2. mock-build\t1.0.0' \
  "build path dependency was not published before its dependent crate."
assert_contains "$dev_build_output" $'3. mock-root\t1.0.0' \
  "dependent crate was not published after its dev and build path dependencies."
assert_contains "$dev_build_output" "Cargo crate publish plan is valid." \
  "dev/build dependency publish plan should pass."

if [[ -s "$dev_build_curl_log" ]]; then
  cat "$dev_build_curl_log" >&2
  fail "dev/build publishable graph should not query crates.io versions."
fi

git_only_metadata="${work_dir}/git-only-metadata.json"
write_git_only_metadata "$git_only_metadata"
git_only_output="${work_dir}/git-only-output.txt"
git_only_curl_log="${work_dir}/git-only-curl.log"
: > "$git_only_curl_log"
set +e
run_publish_check "$git_only_metadata" "$exists_file" "$git_only_curl_log" \
  > "$git_only_output" 2>&1
git_only_status=$?
set -e

if [[ "$git_only_status" -eq 0 ]]; then
  fail "git-only dependency reachable during packaging should fail."
fi
assert_contains "$git_only_output" "Publishable crates must resolve dependencies from crates.io." \
  "git-only dependency failure did not explain the crates.io source requirement."
assert_contains "$git_only_output" "git+https://github.com/example/ecgfp5" \
  "git-only dependency failure did not print the blocked source."
assert_not_contains "$git_only_output" "Cargo crate publish plan is valid." \
  "git-only dependency failure should not report a valid publish plan."

if [[ -s "$git_only_curl_log" ]]; then
  cat "$git_only_curl_log" >&2
  fail "git-only dependency source failure should not query crates.io versions."
fi

cycle_metadata="${work_dir}/cycle-metadata.json"
write_cycle_metadata "$cycle_metadata"
cycle_output="${work_dir}/cycle-output.txt"
cycle_curl_log="${work_dir}/cycle-curl.log"
: > "$cycle_curl_log"
set +e
run_publish_check "$cycle_metadata" "$exists_file" "$cycle_curl_log" \
  > "$cycle_output" 2>&1
cycle_status=$?
set -e

if [[ "$cycle_status" -eq 0 ]]; then
  fail "cyclic publishable path dependencies should fail."
fi
assert_contains "$cycle_output" "unable to sort publishable crates" \
  "cycle failure did not report the topological sort error."
assert_contains "$cycle_output" "mock-cycle-a waits for mock-cycle-b" \
  "cycle failure did not report the first blocked crate."
assert_contains "$cycle_output" "mock-cycle-b waits for mock-cycle-a" \
  "cycle failure did not report the second blocked crate."

if [[ -s "$cycle_curl_log" ]]; then
  cat "$cycle_curl_log" >&2
  fail "cycle failure should not query crates.io versions."
fi

empty_plan_metadata="${work_dir}/empty-plan-metadata.json"
write_empty_plan_metadata "$empty_plan_metadata"
empty_plan_output="${work_dir}/empty-plan-output.txt"
empty_plan_curl_log="${work_dir}/empty-plan-curl.log"
: > "$empty_plan_curl_log"
set +e
run_publish_check "$empty_plan_metadata" "$exists_file" "$empty_plan_curl_log" \
  > "$empty_plan_output" 2>&1
empty_plan_status=$?
set -e

if [[ "$empty_plan_status" -eq 0 ]]; then
  fail "metadata with no publishable workspace crates should fail."
fi
assert_contains "$empty_plan_output" "No publishable workspace crates found." \
  "empty publish plan failure did not report the missing publishable crates."

if [[ -s "$empty_plan_curl_log" ]]; then
  cat "$empty_plan_curl_log" >&2
  fail "empty publish plan failure should not query crates.io versions."
fi

blocked_dependency_metadata="${work_dir}/blocked-dependency-metadata.json"
write_crates_io_dependency_metadata "$blocked_dependency_metadata"
blocked_dependency_output="${work_dir}/blocked-dependency-output.txt"
blocked_dependency_curl_log="${work_dir}/blocked-dependency-curl.log"
: > "$blocked_dependency_curl_log"
set +e
run_publish_check "$blocked_dependency_metadata" "$exists_file" "$blocked_dependency_curl_log" \
  > "$blocked_dependency_output" 2>&1
blocked_dependency_status=$?
set -e

if [[ "$blocked_dependency_status" -eq 0 ]]; then
  fail "publish=false path dependency absent from crates.io should fail."
fi
assert_contains "$blocked_dependency_output" \
  "mock-existing-private 0.3.0 is marked publish=false and is absent from crates.io." \
  "blocked dependency failure did not explain the missing crates.io version."
assert_contains "$blocked_dependency_curl_log" \
  "https://crates.io/api/v1/crates/mock-existing-private/0.3.0" \
  "blocked dependency failure did not check the local dependency version on crates.io."

printf '%s\n' "mock-existing-private 0.3.0 503" > "$error_file"
registry_error_output="${work_dir}/registry-error-output.txt"
registry_error_curl_log="${work_dir}/registry-error-curl.log"
: > "$registry_error_curl_log"
set +e
run_publish_check "$blocked_dependency_metadata" "$exists_file" "$registry_error_curl_log" \
  > "$registry_error_output" 2>&1
registry_error_status=$?
set -e

if [[ "$registry_error_status" -ne 2 ]]; then
  fail "crates.io HTTP error exit status was ${registry_error_status}, expected 2."
fi
assert_contains "$registry_error_output" \
  "crates.io lookup failed for mock-existing-private 0.3.0 (HTTP 503)." \
  "crates.io HTTP error did not surface the registry lookup failure."
: > "$error_file"

printf '%s\n' "mock-existing-private 0.3.0" > "$exists_file"
crates_io_metadata="${work_dir}/crates-io-dependency-metadata.json"
write_crates_io_dependency_metadata "$crates_io_metadata"
crates_io_output="${work_dir}/crates-io-dependency-output.txt"
crates_io_curl_log="${work_dir}/crates-io-dependency-curl.log"
: > "$crates_io_curl_log"
run_publish_check "$crates_io_metadata" "$exists_file" "$crates_io_curl_log" \
  > "$crates_io_output" 2>&1

assert_contains "$crates_io_output" $'1. mock-public\t1.0.0' \
  "valid publish plan did not include the publishable crate."
assert_contains "$crates_io_output" "Cargo crate publish plan is valid." \
  "valid publish plan with an existing crates.io dependency should pass."
assert_contains "$crates_io_curl_log" \
  "https://crates.io/api/v1/crates/mock-existing-private/0.3.0" \
  "valid publish plan did not check the local dependency version on crates.io."

echo "publish cargo crates check tests passed."
