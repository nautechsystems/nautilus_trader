#!/usr/bin/env bash
# Mock script variables expand when the generated files run.
# shellcheck disable=SC2016

set -euo pipefail

REPO_ROOT=$(git rev-parse --show-toplevel)
CASE_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/nautilus-artifact-reuse.XXXXXX")
trap 'rm -rf "$CASE_ROOT"' EXIT

MOCK_BIN="$CASE_ROOT/bin"
UV_LOG="$CASE_ROOT/uv.log"
CARGO_LOG="$CASE_ROOT/cargo.log"
GIT_LOG="$CASE_ROOT/git.log"
MAKE_LOG="$CASE_ROOT/make.log"
TARGET_DIR="$CASE_ROOT/target"
SOURCE_DIR="$CASE_ROOT/source"
MAKE_BIN=$(command -v make)

mkdir -p "$MOCK_BIN" "$SOURCE_DIR"
: > "$SOURCE_DIR/input.rs"
: > "$UV_LOG"

printf '%s\n' \
  '#!/usr/bin/env bash' \
  '' \
  'set -euo pipefail' \
  '' \
  'if [ "${1:-}" = "--version" ]; then' \
  '  echo "uv 0.12.3"' \
  '  exit 0' \
  'fi' \
  '' \
  'printf "%s|stub-profile=%s\n" "$*" "${NAUTILUS_STUB_PROFILE:-}" >> "${UV_LOG:?}"' > "$MOCK_BIN/uv"

printf '%s\n' \
  '#!/usr/bin/env bash' \
  '' \
  'set -euo pipefail' \
  '' \
  'printf "%s\n" "$*" >> "${CARGO_LOG:?}"' > "$MOCK_BIN/cargo"

chmod +x "$MOCK_BIN/uv" "$MOCK_BIN/cargo"

fail() {
  echo "ERROR: $1" >&2
  exit 1
}

run_make() {
  local inputs="$1"
  local input_list_command="printf '%s\\n' $inputs"
  shift

  PATH="$MOCK_BIN:$PATH" \
    UV_LOG="$UV_LOG" \
    make -C "$REPO_ROOT" --no-print-directory \
    TARGET_DIR="$TARGET_DIR" \
    UV_PROJECT_ENVIRONMENT="$CASE_ROOT/venv" \
    CARGO_CI_PROFILE=nextest \
    PY_STUB_INPUTS="$inputs" \
    PY_STUB_INPUT_LIST_COMMAND="$input_list_command" \
    PYTHON_EXTENSION_PATH="$SOURCE_DIR/_libnautilus.so" \
    "$@" > /dev/null
}

stub_generation_count() {
  grep -Fc "run --no-sync python generate_stubs.py" "$UV_LOG" || true
}

source_inputs="$SOURCE_DIR $SOURCE_DIR/input.rs"
run_make "$source_inputs" py-stubs
[[ -f "$TARGET_DIR/.py-stubs.stamp" ]] || fail "Stub generation did not create its stamp"
[[ "$(stub_generation_count)" -eq 1 ]] || fail "Initial stub generation did not run once"
grep -Fq "run --no-sync python generate_stubs.py|stub-profile=nextest" "$UV_LOG" ||
  fail "Stub generation did not use the nextest profile"

run_make "$source_inputs" py-stubs
[[ "$(stub_generation_count)" -eq 1 ]] || fail "Fresh stub inputs triggered regeneration"

sleep 1
touch "$SOURCE_DIR/input.rs"
run_make "$source_inputs" py-stubs
[[ "$(stub_generation_count)" -eq 2 ]] || fail "A changed stub input did not trigger regeneration"

sleep 1
rm "$SOURCE_DIR/input.rs"
run_make "" py-stubs
[[ "$(stub_generation_count)" -eq 3 ]] || fail "A deleted stub input did not trigger regeneration"

run_make "$SOURCE_DIR" pytest-collect-fast
grep -Fq "run --no-sync pytest tests/ --collect-only -q" "$UV_LOG" ||
  fail "Fast Python collection did not collect the test tree"
if grep -Fq "maturin develop" "$UV_LOG"; then
  fail "Fast Python collection rebuilt the extension"
fi

collection_count=$(grep -Fc "run --no-sync pytest tests/ --collect-only -q" "$UV_LOG")
run_make "$SOURCE_DIR" PYTHON_EXTENSION_PATH= pytest-collect-fast
[[ "$(grep -Fc "run --no-sync pytest tests/ --collect-only -q" "$UV_LOG")" -eq "$collection_count" ]] ||
  fail "Fast Python collection ran without an existing extension"

run_make "$SOURCE_DIR" pytest
grep -Fq "run --no-sync maturin develop --profile nextest" "$UV_LOG" ||
  fail "Full Python tests did not build with the nextest profile"

grep -Fq -- "- id: python-test-collection" "$REPO_ROOT/.pre-commit-config.yaml" ||
  fail "Python collection is not registered with pre-commit"
grep -Fq "entry: make pytest-collect-fast" "$REPO_ROOT/.pre-commit-config.yaml" ||
  fail "Pre-commit does not use fast Python collection"

printf '%s\n' \
  '#!/usr/bin/env bash' \
  '' \
  'set -euo pipefail' \
  '' \
  'if [ "${1:-}" = "diff" ]; then' \
  '  exit 0' \
  'fi' \
  '' \
  'echo "Unexpected git command: $*" >&2' \
  'exit 1' > "$MOCK_BIN/git"
chmod +x "$MOCK_BIN/git"

PATH="$MOCK_BIN:$PATH" \
  CARGO_CI_PROFILE=nextest \
  CARGO_LOG="$CARGO_LOG" \
  CHANGED_BASE_SHA='' \
  bash "$REPO_ROOT/scripts/clippy-changed.sh"
grep -Fq \
  "clippy --workspace --lib --bins --tests --features arrow,ffi,python,high-precision,streaming,defi --profile nextest -- -D warnings" \
  "$CARGO_LOG" || fail "Clippy features do not match make check-code"

printf '%s\n' \
  '#!/usr/bin/env bash' \
  '' \
  'set -euo pipefail' \
  '' \
  'printf "%s\n" "$*" >> "${GIT_LOG:?}"' \
  'if [ "${1:-}" = "rev-parse" ]; then' \
  '  echo develop' \
  '  exit 0' \
  'fi' \
  'if [ "${1:-}" != "diff" ]; then' \
  '  echo "Unexpected git command: $*" >&2' \
  '  exit 1' \
  'fi' \
  'if [ "${POSTGRES_INPUT_CHANGED:?}" = "true" ]; then' \
  '  exit 1' \
  'fi' \
  'exit 0' > "$MOCK_BIN/git"

printf '%s\n' \
  '#!/usr/bin/env bash' \
  '' \
  'set -euo pipefail' \
  '' \
  'printf "%s\n" "$*" >> "${MAKE_LOG:?}"' > "$MOCK_BIN/make"
chmod +x "$MOCK_BIN/git" "$MOCK_BIN/make"

: > "$GIT_LOG"
: > "$MAKE_LOG"
PATH="$MOCK_BIN:$PATH" \
  GIT_LOG="$GIT_LOG" \
  MAKE_LOG="$MAKE_LOG" \
  POSTGRES_INPUT_CHANGED=false \
  "$MAKE_BIN" -C "$REPO_ROOT" --no-print-directory \
  MAKE="$MOCK_BIN/make" cargo-test-postgres-changed > /dev/null
[[ ! -s "$MAKE_LOG" ]] || fail "PostgreSQL bootstrap tests ran without related changes"
grep -Fq \
  "diff --cached --quiet -- schema/sql crates/infrastructure/src/sql/pg.rs crates/infrastructure/tests/test_cache_database_postgres.rs crates/cli/src/database crates/cli/src/bin/cli.rs crates/cli/src/lib.rs crates/cli/src/opt.rs scripts/ci/test-postgres-bootstrap.bash" \
  "$GIT_LOG" || fail "PostgreSQL bootstrap change detection does not cover its inputs"

PATH="$MOCK_BIN:$PATH" \
  GIT_LOG="$GIT_LOG" \
  MAKE_LOG="$MAKE_LOG" \
  POSTGRES_INPUT_CHANGED=true \
  "$MAKE_BIN" -C "$REPO_ROOT" --no-print-directory \
  MAKE="$MOCK_BIN/make" cargo-test-postgres-changed > /dev/null
grep -Fxq -- "--no-print-directory cargo-test-postgres-ci" "$MAKE_LOG" ||
  fail "Related changes did not trigger PostgreSQL bootstrap tests"

echo "Build artifact reuse script tests passed"
