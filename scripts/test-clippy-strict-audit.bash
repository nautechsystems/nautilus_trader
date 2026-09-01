#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
CASE_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/nautilus-clippy-strict.XXXXXX")"
trap 'rm -rf "$CASE_ROOT"' EXIT

FAKE_BIN="${CASE_ROOT}/bin"
CARGO_LOG="${CASE_ROOT}/cargo.log"
REPORT="${CASE_ROOT}/report.md"
ERROR_LOG="${CASE_ROOT}/error.log"
MAKE_BIN="$(command -v make)"
mkdir -p "$FAKE_BIN"

cat > "${FAKE_BIN}/cargo" << 'FAKE_CARGO'
#!/usr/bin/env bash
set -euo pipefail

printf '%s\n' "$*" >> "${CARGO_LOG:?}"

if [[ "${1:-}" == "metadata" ]]; then
  printf '%s\n' '{"packages":[{"id":"core-id","name":"nautilus-core"},{"id":"model-id","name":"nautilus-model"}]}'
  exit 0
fi

if [[ "${1:-}" != "clippy" ]]; then
  echo "Unexpected Cargo command: $*" >&2
  exit 2
fi

if [[ "${FAKE_CARGO_BAD_JSON:-0}" == "1" ]]; then
  echo 'not JSON'
  exit 0
fi

if [[ " $* " == *" --tests "* ]]; then
  if [[ "${FAKE_CARGO_FAIL_TESTS:-0}" == "1" ]]; then
    exit 17
  fi
  printf '%s\n' \
    '{"reason":"compiler-message","package_id":"core-id","message":{"code":{"code":"clippy::unwrap_used"},"spans":[{"file_name":"crates/core/src/lib.rs","line_start":10,"column_start":5,"is_primary":true}]}}' \
    '{"reason":"compiler-message","package_id":"core-id","message":{"code":{"code":"clippy::panic"},"spans":[{"file_name":"crates/core/src/lib.rs","line_start":20,"column_start":7,"is_primary":true}]}}' \
    '{"reason":"compiler-message","package_id":"model-id","message":{"code":{"code":"clippy::indexing_slicing"},"spans":[{"file_name":"crates/model/tests/model.rs","line_start":30,"column_start":9,"is_primary":true}]}}' \
    '{"reason":"compiler-message","package_id":"model-id","message":{"code":{"code":"clippy::expect_used"},"spans":[{"file_name":"crates/model/tests/model.rs","line_start":40,"column_start":11,"is_primary":true}]}}'
else
  printf '%s\n' \
    '{"reason":"compiler-message","package_id":"core-id","message":{"code":{"code":"clippy::unwrap_used"},"spans":[{"file_name":"crates/core/src/lib.rs","line_start":10,"column_start":5,"is_primary":true}]}}' \
    '{"reason":"compiler-message","package_id":"core-id","message":{"code":{"code":"clippy::unwrap_used"},"spans":[{"file_name":"crates/core/src/lib.rs","line_start":10,"column_start":5,"is_primary":true}]}}' \
    '{"reason":"compiler-message","package_id":"core-id","message":{"code":{"code":"clippy::panic"},"spans":[{"file_name":"crates/core/src/lib.rs","line_start":20,"column_start":7,"is_primary":true}]}}'
fi
FAKE_CARGO
chmod +x "${FAKE_BIN}/cargo"

fail() {
  echo "ERROR: $1" >&2
  exit 1
}

PATH="${FAKE_BIN}:${PATH}" CARGO_LOG="$CARGO_LOG" \
  "$MAKE_BIN" -C "$REPO_ROOT" --no-print-directory \
  CARGO_CI_PROFILE=nextest DEFI=true EXTRA_FEATURES= \
  clippy-strict-audit > "$REPORT" 2> "$ERROR_LOG"

[[ "$(grep -c '^clippy ' "$CARGO_LOG")" -eq 2 ]] || fail "Audit did not run Clippy twice"
grep -Fq \
  'clippy --quiet --workspace --locked --lib --bins --features arrow,ffi,python,high-precision,streaming,defi --profile nextest --no-deps --color never --message-format=json --' \
  "$CARGO_LOG" || fail "Production audit command changed"
grep -Fq \
  'clippy --quiet --workspace --locked --lib --bins --tests --features arrow,ffi,python,high-precision,streaming,defi --profile nextest --no-deps --color never --message-format=json --' \
  "$CARGO_LOG" || fail "Test audit command changed"
grep -Fq -- '--force-warn clippy::unimplemented' "$CARGO_LOG" ||
  fail "Audit did not force candidate lints to warnings"

grep -Fq "| \`unwrap_used\` | 1 | 0 | 1 |" "$REPORT" || fail "Production count was wrong"
grep -Fq "| \`panic\` | 1 | 0 | 1 |" "$REPORT" || fail "Duplicate count was wrong"
grep -Fq "| \`indexing_slicing\` | 0 | 1 | 1 |" "$REPORT" || fail "Test count was wrong"
grep -Fq "| \`expect_used\` | 0 | 1 | 1 |" "$REPORT" || fail "Test count was wrong"
grep -Fq '| **Total** | **2** | **2** | **4** |' "$REPORT" || fail "Totals were wrong"
grep -Fq "| \`nautilus-core\` | \`panic\` | 1 |" "$REPORT" ||
  fail "Production package breakdown was missing"
grep -Fq "| \`nautilus-model\` | \`indexing_slicing\` | 1 |" "$REPORT" ||
  fail "Test package breakdown was missing"

: > "$CARGO_LOG"
PATH="${FAKE_BIN}:${PATH}" CARGO_LOG="$CARGO_LOG" \
  "$MAKE_BIN" -C "$REPO_ROOT" --no-print-directory \
  clippy-pedantic-crate-nautilus-core > /dev/null
grep -Fq \
  'clippy --all-targets --all-features -p nautilus-core -- -D warnings -W clippy::pedantic -W clippy::todo -W clippy::unwrap_used -W clippy::expect_used' \
  "$CARGO_LOG" || fail "Pedantic crate target did not enable its named lint group"

if PATH="${FAKE_BIN}:${PATH}" CARGO_LOG="$CARGO_LOG" FAKE_CARGO_FAIL_TESTS=1 \
  "$MAKE_BIN" -C "$REPO_ROOT" --no-print-directory \
  CARGO_CI_PROFILE=nextest DEFI=true EXTRA_FEATURES= \
  clippy-strict-audit > "$REPORT" 2> "$ERROR_LOG"; then
  fail "Audit ignored a Cargo failure"
fi
grep -Fq 'test Cargo Clippy run failed with status 17' "$ERROR_LOG" ||
  fail "Audit did not explain the Cargo failure"

if PATH="${FAKE_BIN}:${PATH}" CARGO_LOG="$CARGO_LOG" FAKE_CARGO_BAD_JSON=1 \
  "$MAKE_BIN" -C "$REPO_ROOT" --no-print-directory \
  CARGO_CI_PROFILE=nextest DEFI=true EXTRA_FEATURES= \
  clippy-strict-audit > "$REPORT" 2> "$ERROR_LOG"; then
  fail "Audit accepted malformed Cargo output"
fi
grep -Fq 'Cargo Clippy returned invalid JSON' "$ERROR_LOG" ||
  fail "Audit did not explain the malformed output"

echo "Strict Clippy audit tests passed"
