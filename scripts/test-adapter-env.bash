#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
case_root=$(mktemp -d "${TMPDIR:-/tmp}/nautilus-adapter-env.XXXXXX")
trap 'rm -rf "$case_root"' EXIT
mkdir "$case_root/bin"

awk '
  /^ADAPTER_ENV_VARS=\(/ { inside = 1; next }
  inside && /^\)/ { exit }
  inside && $1 ~ /^[A-Z][A-Z0-9_]*$/ { print $1 }
' "$repo_root/scripts/strip-adapter-env.bash" > "$case_root/variables"
grep -Fxq POLYMARKET_FUNDER "$case_root/variables"

while IFS= read -r name; do
  export "$name=adapter-env-test"
done < "$case_root/variables"

export ADAPTER_ENV_VARIABLES="$case_root/variables"
export ADAPTER_ENV_LOG="$case_root/commands"
export ADAPTER_ENV_CONTROL='preserved value'
export CARGO_BUILD_JOBS=2
export NEXTEST_TEST_THREADS=2

cat > "$case_root/bin/cargo" << 'PROBE'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == metadata ]]; then
  printf '%s\n' '{"packages":[{"name":"nautilus-model","features":{}}]}'
  exit 0
fi
while IFS= read -r name; do
  if [[ "${!name+x}" == x ]]; then
    echo "ERROR: Test runner inherited $name" >&2
    exit 19
  fi
done < "${ADAPTER_ENV_VARIABLES:?}"
[[ "${ADAPTER_ENV_CONTROL:-}" == 'preserved value' ]]
[[ "${CARGO_BUILD_JOBS:-}" == 2 ]]
[[ "${NEXTEST_TEST_THREADS:-}" == 2 ]]
printf '%s\n' "$*" >> "${ADAPTER_ENV_LOG:?}"
exit "${ADAPTER_ENV_STATUS:-0}"
PROBE
cp "$case_root/bin/cargo" "$case_root/bin/uv"
chmod +x "$case_root/bin/cargo" "$case_root/bin/uv"
export PATH="$case_root/bin:$PATH"

status=0
cargo nextest run > "$case_root/output" 2>&1 || status=$?
[[ "$status" == 19 ]]
grep -Fq 'ERROR: Test runner inherited' "$case_root/output"

bash "$repo_root/scripts/strip-adapter-env.bash" cargo nextest run
bash "$repo_root/scripts/strip-adapter-env.bash" bash -c \
  '[[ "$1" == "argument with spaces" && "$2" == "" ]]' _ 'argument with spaces' ''
status=0
bash "$repo_root/scripts/strip-adapter-env.bash" bash -c 'exit 37' || status=$?
[[ "$status" == 37 ]]

run_make() {
  local make_command=(make
    -o check-cargo-cooldown -o check-nextest-installed -o check-llvm-cov-installed
    -o check-miri-installed -o build-debug)
  "${make_command[@]}" -C "$repo_root" --no-print-directory \
    MAKE="${make_command[*]}" \
    CARGO_BUILD_JOBS="$CARGO_BUILD_JOBS" NEXTEST_TEST_THREADS="$NEXTEST_TEST_THREADS" \
    PYTHON_EXTENSION_PATH=existing-extension.so "$@"
}

for target in \
  cargo-test cargo-test-extras cargo-test-doc cargo-test-core-local \
  cargo-test-core-selected cargo-test-core cargo-test-adapters cargo-test-sim \
  cargo-test-core-debug cargo-test-core-local-debug cargo-test-lib \
  cargo-test-standard-precision cargo-test-debug cargo-test-coverage \
  cargo-test-crate-nautilus-model cargo-test-coverage-crate-nautilus-model \
  cargo-test-coverage-html cargo-test-coverage-crate-html-nautilus-model \
  cargo-miri-core cargo-miri-core-ffi cargo-miri-model cargo-miri-plugin cargo-miri \
  pytest-collect-fast pytest pytest-memray; do
  for verbose in false true; do
    : > "$ADAPTER_ENV_LOG"
    if ! run_make "$target" NEXTEST_VERBOSE="$verbose" > "$case_root/output" 2>&1; then
      echo "ERROR: Environment isolation failed for $target (verbose=$verbose)" >&2
      cat "$case_root/output" >&2
      exit 1
    fi
    if [[ ! -s "$ADAPTER_ENV_LOG" ]]; then
      echo "ERROR: No test runner was called for $target" >&2
      exit 1
    fi
  done
done

if ! MAKEFLAGS=' -- CARGO_BUILD_JOBS=4 NEXTEST_TEST_THREADS=4' \
  run_make cargo-test > "$case_root/output" 2>&1; then
  echo 'ERROR: Inherited Make overrides changed the isolation test controls' >&2
  cat "$case_root/output" >&2
  exit 1
fi

for target in cargo-test-extras cargo-test-doc; do
  status=0
  ADAPTER_ENV_STATUS=23 run_make "$target" > "$case_root/output" 2>&1 || status=$?
  if [[ "$status" != 2 ]] || ! grep -Fq 'Error 23' "$case_root/output"; then
    echo "ERROR: $target did not preserve the test runner failure" >&2
    cat "$case_root/output" >&2
    exit 1
  fi
done

for target in cargo-test-postgres-ci pytest-isolated pytest-doctest; do
  run_make --dry-run "$target" > "$case_root/output"
  if ! grep -F "scripts/strip-adapter-env.bash\" bash scripts/" "$case_root/output" > /dev/null; then
    echo "ERROR: $target does not isolate its test script" >&2
    cat "$case_root/output" >&2
    exit 1
  fi
done

echo 'Adapter environment isolation tests passed'
