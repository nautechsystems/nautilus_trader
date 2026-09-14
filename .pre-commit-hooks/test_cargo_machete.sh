#!/usr/bin/env bash
# Mock script variables expand when the generated files run.
# shellcheck disable=SC2016

set -euo pipefail

REPO_ROOT=$(git rev-parse --show-toplevel)
cd "$REPO_ROOT"

HOOK="$REPO_ROOT/.pre-commit-hooks/cargo_machete.sh"
CASE_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/nautilus-cargo-machete.XXXXXX")
trap 'rm -rf "$CASE_ROOT"' EXIT

MOCK_BIN="$CASE_ROOT/bin"
MACHETE_BIN="$CASE_ROOT/machete-bin"
CARGO_LOG="$CASE_ROOT/cargo.log"
GIT_LOG="$CASE_ROOT/git.log"
OUTPUT="$CASE_ROOT/output.log"

mkdir -p "$MOCK_BIN" "$MACHETE_BIN"

printf '%s\n' \
  '#!/usr/bin/env bash' \
  '' \
  'set -euo pipefail' \
  '' \
  'if [ "${1:-}" = "machete" ] && [ "${2:-}" = "--version" ]; then' \
  '  echo "0.9.2"' \
  '  exit 0' \
  'fi' \
  '' \
  'printf "%s\n" "$*" >> "${CARGO_LOG:?}"' \
  'exit "${MACHETE_STATUS:-0}"' > "$MOCK_BIN/cargo"

printf '%s\n' \
  '#!/usr/bin/env bash' \
  '' \
  'exit 0' > "$MACHETE_BIN/cargo-machete"

printf '%s\n' \
  '#!/usr/bin/env bash' \
  '' \
  'set -euo pipefail' \
  '' \
  'printf "%s\n" "$*" >> "${GIT_LOG:?}"' \
  'case "${1:-}" in' \
  '  diff)' \
  '    case " $* " in' \
  '      *" resolved-base..HEAD "*) changed_files="${GIT_BASE_CHANGED_FILES:-}" ;;' \
  '      *) changed_files="${GIT_CHANGED_FILES:-}" ;;' \
  '    esac' \
  '    if [ -n "$changed_files" ]; then' \
  '      printf "%s\n" "$changed_files"' \
  '    fi' \
  '    ;;' \
  '  merge-base)' \
  '    if [ "${GIT_BASE_RESOLVES:-true}" = "true" ]; then' \
  '      echo "resolved-base"' \
  '    else' \
  '      exit 1' \
  '    fi' \
  '    ;;' \
  '  *)' \
  '    echo "Unexpected git command: $*" >&2' \
  '    exit 1' \
  '    ;;' \
  'esac' > "$MOCK_BIN/git"

chmod +x "$MOCK_BIN/cargo" "$MACHETE_BIN/cargo-machete" "$MOCK_BIN/git"

fail() {
  echo "ERROR: $1" >&2
  if [ -s "$OUTPUT" ]; then
    cat "$OUTPUT" >&2
  fi
  exit 1
}

run_hook() {
  local changed_files="$1"
  local changed_base="${2:-}"
  local machete_status="${3:-0}"
  local base_changed_files="${4:-}"
  local base_resolves="${5:-true}"
  local tool_available="${6:-true}"
  local hook_path="$MOCK_BIN:/usr/bin:/bin"

  if [ "$tool_available" = "true" ]; then
    hook_path="$MACHETE_BIN:$hook_path"
  fi

  : > "$CARGO_LOG"
  : > "$GIT_LOG"
  : > "$OUTPUT"

  set +e
  PATH="$hook_path" \
    CARGO_LOG="$CARGO_LOG" \
    GIT_LOG="$GIT_LOG" \
    GIT_CHANGED_FILES="$changed_files" \
    GIT_BASE_CHANGED_FILES="$base_changed_files" \
    GIT_BASE_RESOLVES="$base_resolves" \
    CHANGED_BASE_SHA="$changed_base" \
    MACHETE_STATUS="$machete_status" \
    bash "$HOOK" > "$OUTPUT" 2>&1
  RUN_STATUS=$?
  set -e
}

assert_passed() {
  [ "$RUN_STATUS" -eq 0 ] || fail "Cargo Machete hook failed"
}

assert_cargo_log() {
  local expected="$1"
  local actual

  actual=$(cat "$CARGO_LOG")
  [ "$actual" = "$expected" ] || fail "Unexpected Cargo command: $actual"
}

run_hook "crates/model/src/order.rs"
assert_passed
assert_cargo_log "machete --with-metadata crates/model/Cargo.toml"
grep -Fq -- "diff --cached --name-only --no-renames" "$GIT_LOG" ||
  fail "Staged change detection does not preserve both sides of renames"

run_hook "$(printf '%s\n' crates/model/Cargo.toml crates/model/src/order.rs)"
assert_passed
assert_cargo_log "machete --with-metadata crates/model/Cargo.toml"

run_hook "$(printf '%s\n' crates/core/src/lib.rs crates/model/src/order.rs)"
assert_passed
assert_cargo_log "machete --with-metadata crates/core/Cargo.toml crates/model/Cargo.toml"

run_hook "crates/adapters/lighter/fuzz/pornin/fuzz_targets/fuzz_pornin_diff_decode.rs"
assert_passed
assert_cargo_log "machete --with-metadata crates/adapters/lighter/fuzz/pornin/Cargo.toml"

run_hook "examples/quickstarts/lighter-rust-data-client/src/main.rs"
assert_passed
assert_cargo_log "machete examples/quickstarts/lighter-rust-data-client/Cargo.toml"

run_hook "$(printf '%s\n' crates/model/src/order.rs examples/quickstarts/lighter-rust-data-client/src/main.rs)"
assert_passed
assert_cargo_log "$(printf '%s\n' \
  'machete --with-metadata crates/model/Cargo.toml' \
  'machete examples/quickstarts/lighter-rust-data-client/Cargo.toml')"

full_commands=$(printf '%s\n' \
  'machete --with-metadata Cargo.toml crates examples/tutorials' \
  'machete examples/quickstarts/lighter-rust-data-client')

for changed_file in Cargo.toml .pre-commit-hooks/cargo_machete.sh crates/missing/Cargo.toml; do
  run_hook "$changed_file"
  assert_passed
  assert_cargo_log "$full_commands"
done

run_hook "$(printf '%s\n' docs/dev_templates/criterion_template.rs patches/arrow-compat/src/lib.rs)"
assert_passed
[ ! -s "$CARGO_LOG" ] || fail "Unmaintained Rust paths triggered cargo machete"
grep -Fq "No maintained Rust package changes detected; skipping cargo machete" "$OUTPUT" ||
  fail "Unmaintained Rust path skip was not reported"

run_hook "docs/developer_guide/testing.md" "" "0" "" "true" "false"
assert_passed
[ ! -s "$CARGO_LOG" ] || fail "A docs-only change without Cargo Machete invoked cargo"

run_hook "crates/model/src/order.rs" "" "0" "" "true" "false"
[ "$RUN_STATUS" -eq 1 ] || fail "A Rust change passed without Cargo Machete installed"
grep -Fq "cargo-machete 0.9.2 is required" "$OUTPUT" ||
  fail "A missing Cargo Machete install was not reported"

run_hook "" "base-sha" "0" "crates/model/src/order.rs"
assert_passed
assert_cargo_log "machete --with-metadata crates/model/Cargo.toml"

run_hook "" "base-sha"
assert_passed
[ ! -s "$CARGO_LOG" ] || fail "An empty resolved diff triggered cargo machete"
grep -Fq "No changes detected; skipping cargo machete" "$OUTPUT" ||
  fail "Empty resolved diff skip was not reported"

run_hook "" "base-sha" "0" "" "false"
assert_passed
assert_cargo_log "$full_commands"

run_hook ""
assert_passed
assert_cargo_log "$full_commands"

run_hook "crates/model/src/order.rs" "" "1"
[ "$RUN_STATUS" -eq 1 ] || fail "Cargo Machete failure was not propagated"
grep -Fq "[package.metadata.cargo-machete]" "$OUTPUT" ||
  fail "Cargo Machete failure guidance was not reported"

cargo_machete_config=$(awk '
  /^      - id: cargo-machete$/ {
    capture = 1
  }
  capture && /^      - id:/ && !/^      - id: cargo-machete$/ {
    exit
  }
  capture {
    print
  }
' "$REPO_ROOT/.pre-commit-config.yaml")
[[ "$cargo_machete_config" == *"always_run: true"* ]] ||
  fail "Cargo Machete hook does not run for deletion-only changes"

echo "Cargo Machete hook tests passed"
