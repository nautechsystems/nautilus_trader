#!/usr/bin/env bash
# Mock script variables expand when the generated file runs.
# shellcheck disable=SC2016

set -euo pipefail

REPO_ROOT=$(git rev-parse --show-toplevel)
CASE_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/nautilus-uv-project-env.XXXXXX")
trap 'rm -rf "$CASE_ROOT"' EXIT

MOCK_BIN="$CASE_ROOT/bin"
UV_LOG="$CASE_ROOT/uv.log"

mkdir -p "$MOCK_BIN"
: > "$UV_LOG"

printf '%s\n' \
  '#!/usr/bin/env bash' \
  '' \
  'set -euo pipefail' \
  '' \
  'printf "%s\n" "${UV_PROJECT_ENVIRONMENT:-<unset>}" >> "${UV_LOG:?}"' > "$MOCK_BIN/uv"

chmod +x "$MOCK_BIN/uv"

fail() {
  echo "ERROR: $1" >&2
  exit 1
}

resolved_environment() {
  : > "$UV_LOG"
  (
    cd "$CASE_ROOT"
    PATH="$MOCK_BIN:$PATH" UV_LOG="$UV_LOG" "$@" > /dev/null 2>&1 || true
  )
  head -n 1 "$UV_LOG"
}

# The type check defaults to the repository-root environment, from any working directory.
actual=$(resolved_environment env -u UV_PROJECT_ENVIRONMENT \
  bash "$REPO_ROOT/scripts/ci/check-python-types.bash" "$REPO_ROOT/python" "$REPO_ROOT/examples")
[ "$actual" = "$REPO_ROOT/.venv" ] ||
  fail "check-python-types.bash resolved '$actual', expected '$REPO_ROOT/.venv'"

# An explicit value wins, which is how the wheel test targets the installed wheel.
actual=$(resolved_environment env UV_PROJECT_ENVIRONMENT="$CASE_ROOT/preset" \
  bash "$REPO_ROOT/scripts/ci/check-python-types.bash" "$REPO_ROOT/python" "$REPO_ROOT/examples")
[ "$actual" = "$CASE_ROOT/preset" ] ||
  fail "check-python-types.bash overrode a preset value with '$actual'"

# The doctest runner follows the same rules.
actual=$(resolved_environment env -u UV_PROJECT_ENVIRONMENT \
  bash "$REPO_ROOT/scripts/ci/test-python-doctests.bash" "$REPO_ROOT/python")
[ "$actual" = "$REPO_ROOT/.venv" ] ||
  fail "test-python-doctests.bash resolved '$actual', expected '$REPO_ROOT/.venv'"

actual=$(resolved_environment env UV_PROJECT_ENVIRONMENT="$CASE_ROOT/preset" \
  bash "$REPO_ROOT/scripts/ci/test-python-doctests.bash" "$REPO_ROOT/python")
[ "$actual" = "$CASE_ROOT/preset" ] ||
  fail "test-python-doctests.bash overrode a preset value with '$actual'"

echo "uv project environment resolution tests passed"
