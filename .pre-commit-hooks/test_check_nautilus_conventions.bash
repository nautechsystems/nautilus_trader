#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)
HOOK="$SCRIPT_DIR/check_nautilus_conventions.sh"
CASE_ROOT=""

main() {
  command -v rg > /dev/null
  TEST_REAL_COMM=$(command -v comm)
  TEST_REAL_SORT=$(command -v sort)
  CASE_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/nautilus-module-comparison.XXXXXX")
  trap 'rm -rf "$CASE_ROOT"' EXIT

  mkdir -p "$CASE_ROOT/repo/crates/pyo3/src" "$CASE_ROOT/repo/examples" \
    "$CASE_ROOT/repo/python" "$CASE_ROOT/bin"
  export TEST_REAL_COMM TEST_REAL_SORT
  export TEST_COMM_CALLS="$CASE_ROOT/comm_calls"
  export TEST_SORT_CALLS="$CASE_ROOT/sort_calls"
  export TEST_COMM_FAIL_MODE="" TEST_COMM_PARTIAL=0 TEST_COMM_STATUS=0 TEST_SORT_FAIL_CALL=0
  write_fault_tools

  reset_fixture
  run_hook
  grep -Fq 'Python submodule registrations are unchanged' "$CASE_ROOT/output"
  grep -Fq 'All Nautilus conventions are valid' "$CASE_ROOT/output"

  reset_fixture
  printf '%s\n' \
    'let n = "sample_extra";' \
    'let submodule = pyo3::wrap_pymodule!(nautilus_sample_extra::python::sample_extra);' \
    >> "$CASE_ROOT/repo/crates/pyo3/src/lib.rs"
  expect_registration_failure 'Unexpected Python submodule registration'

  reset_fixture
  sed '/let n = "analysis";/d; /pyo3::wrap_pymodule!(nautilus_analysis::python::analysis)/d' \
    "$REPO_ROOT/crates/pyo3/src/lib.rs" > "$CASE_ROOT/repo/crates/pyo3/src/lib.rs"
  expect_registration_failure 'Python submodule registration missing'

  local mode partial_output failure_status status
  for mode in -13 -23; do
    for partial_output in 0 1; do
      for failure_status in 1 17; do
        reset_fixture
        status=0
        TEST_COMM_FAIL_MODE="$mode" TEST_COMM_PARTIAL="$partial_output" \
          TEST_COMM_STATUS="$failure_status" run_hook || status=$?
        if [[ "$status" -ne "$failure_status" ]]; then
          cat "$CASE_ROOT/output"
          echo "comm $mode failure returned $status instead of $failure_status"
          exit 1
        fi
        grep -Fq "Injected comm $mode failure" "$CASE_ROOT/output"
        grep -Fxq -- "$mode" "$TEST_COMM_CALLS"
        assert_no_module_result
      done
    done
  done

  local failure_call
  for failure_call in 1 2; do
    reset_fixture
    status=0
    TEST_SORT_FAIL_CALL="$failure_call" run_hook || status=$?
    if [[ "$status" -ne 19 ]]; then
      cat "$CASE_ROOT/output"
      echo "Inventory sort $failure_call failure returned $status instead of 19"
      exit 1
    fi
    grep -Fq 'Injected sort failure' "$CASE_ROOT/output"
    [[ "$(cat "$TEST_SORT_CALLS")" -eq "$failure_call" ]]
    [[ ! -s "$TEST_COMM_CALLS" ]]
    assert_no_module_result
  done

  echo 'Nautilus module-comparison tests passed'
}

write_fault_tools() {
  cat > "$CASE_ROOT/bin/comm" << 'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$1" >> "$TEST_COMM_CALLS"
if [[ "$1" == "$TEST_COMM_FAIL_MODE" ]]; then
  if [[ "$TEST_COMM_PARTIAL" -eq 1 ]]; then
    printf '%s\n' 'sample_extra nautilus_sample_extra::python::sample_extra'
  fi
  echo "Injected comm $1 failure" >&2
  exit "$TEST_COMM_STATUS"
fi
exec "$TEST_REAL_COMM" "$@"
EOF

  cat > "$CASE_ROOT/bin/sort" << 'EOF'
#!/usr/bin/env bash
set -euo pipefail
count=0
if [[ -f "$TEST_SORT_CALLS" ]]; then
  read -r count < "$TEST_SORT_CALLS"
fi
count=$((count + 1))
printf '%s\n' "$count" > "$TEST_SORT_CALLS"
if [[ "$count" -eq "$TEST_SORT_FAIL_CALL" ]]; then
  echo 'Injected sort failure' >&2
  exit 19
fi
exec "$TEST_REAL_SORT" "$@"
EOF
  chmod +x "$CASE_ROOT/bin/comm" "$CASE_ROOT/bin/sort"
}

reset_fixture() {
  cp "$REPO_ROOT/crates/pyo3/src/lib.rs" "$CASE_ROOT/repo/crates/pyo3/src/lib.rs"
  : > "$TEST_COMM_CALLS"
  printf '%s\n' 0 > "$TEST_SORT_CALLS"
}

run_hook() {
  (cd "$CASE_ROOT/repo" && LC_ALL=C PATH="$CASE_ROOT/bin:$PATH" "$BASH" "$HOOK") \
    > "$CASE_ROOT/output" 2>&1
}

expect_registration_failure() {
  local expected="$1" status=0
  run_hook || status=$?
  if [[ "$status" -ne 1 ]]; then
    cat "$CASE_ROOT/output"
    echo "Registration violation returned $status instead of 1"
    exit 1
  fi
  grep -Fq "$expected" "$CASE_ROOT/output"
  if grep -Fq 'Python submodule registrations are unchanged' "$CASE_ROOT/output"; then
    echo 'Nautilus hook reported unchanged registrations after a violation'
    exit 1
  fi
}

assert_no_module_result() {
  local results='Python submodule registrations are unchanged'
  results+='|Unexpected Python submodule registration|Python submodule registration missing'
  results+='|All Nautilus conventions are valid'
  if grep -Eq "$results" "$CASE_ROOT/output"; then
    cat "$CASE_ROOT/output"
    echo 'Nautilus hook interpreted an incomplete comparison as a module result'
    exit 1
  fi
}

main "$@"
