#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)
case_root=$(mktemp -d "${TMPDIR:-/tmp}/nautilus-python-result.XXXXXX")
trap 'rm -rf "$case_root"' EXIT

run_case() {
  local expected_status="$1"
  local expected_output="$2"
  local status=0

  RUN_TESTS="$3" BUILD_RESULT="$4" PLAN_RESULT="$5" \
    PRE_COMMIT_RESULT="$6" SCRIPT_TESTS_RESULT="$7" \
    bash "$repo_root/scripts/ci/check-python-result.bash" > "$case_root/output" 2>&1 || status=$?

  if [ "$status" -ne "$expected_status" ] || [ "$(cat "$case_root/output")" != "$expected_output" ]; then
    echo "Unexpected Python gate result for run-tests=$3, build=$4, plan=$5, pre-commit=$6, script-tests=$7"
    printf 'Expected status: %s\nActual status: %s\n' "$expected_status" "$status"
    printf 'Expected output: %s\nActual output:\n' "$expected_output"
    cat "$case_root/output"
    exit 1
  fi
}

run_case 0 "Python 3.13 build, tests, and generated file checks passed" true success success success success
run_case 0 "Python build and tests intentionally skipped" false skipped success success success

for result in failure cancelled skipped ""; do
  for run_tests in true false; do
    run_case 1 "::error::Python check prerequisites did not succeed" \
      "$run_tests" skipped "$result" success success
    run_case 1 "::error::Python check prerequisites did not succeed" \
      "$run_tests" skipped success "$result" success
    run_case 1 "::error::Python check prerequisites did not succeed" \
      "$run_tests" skipped success success "$result"
  done
done

for run_tests in true false; do
  for result in failure cancelled ""; do
    run_case 1 "::error::Unexpected Python build result: run-tests=$run_tests, build=${result:-unset}" \
      "$run_tests" "$result" success success success
  done
done

run_case 1 "::error::Unexpected Python build result: run-tests=true, build=skipped" \
  true skipped success success success
run_case 1 "::error::Unexpected Python build result: run-tests=false, build=success" \
  false success success success success
run_case 1 "::error::Unexpected Python build result: run-tests=unset, build=skipped" \
  "" skipped success success success
run_case 1 "::error::Unexpected Python build result: run-tests=unknown, build=success" \
  unknown success success success success

echo "Python gate script tests passed"
