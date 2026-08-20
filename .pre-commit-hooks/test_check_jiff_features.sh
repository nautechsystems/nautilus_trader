#!/usr/bin/env bash

set -euo pipefail

for command in jq rg; do
  if ! command -v "$command" &> /dev/null; then
    echo "ERROR: $command is required for Jiff feature hook tests" >&2
    exit 1
  fi
done

REPO_ROOT=$(git rev-parse --show-toplevel)
HOOK="$REPO_ROOT/.pre-commit-hooks/check_jiff_features.sh"

CASE_ROOT=$(mktemp -d)
trap 'rm -rf "$CASE_ROOT"' EXIT

mkdir -p "$CASE_ROOT/bin"
printf '%s\n' \
  '#!/usr/bin/env bash' \
  '' \
  'set -euo pipefail' \
  '' \
  "if [ \"\$1\" = \"metadata\" ]; then" \
  "  if [ \"\${MOCK_JIFF_MODE:-valid}\" = \"patch-violation\" ]; then" \
  '    printf '\''%s\n'\'' '\''{"packages":[{"manifest_path":"/fixture/patches/example/Cargo.toml","dependencies":[{"name":"jiff","rename":null,"uses_default_features":true,"features":[]}]}]}'\''' \
  '  else' \
  '    printf '\''%s\n'\'' '\''{"packages":[]}'\''' \
  '  fi' \
  '  exit 0' \
  'fi' \
  '' \
  "if [ \"\$1\" != \"tree\" ]; then" \
  "  echo \"Unexpected cargo command: \$*\" >&2" \
  '  exit 2' \
  'fi' \
  '' \
  "case \"\$*\" in" \
  '  *"-p nautilus-trading"*)' \
  '    printf '\''jiff feature "%s"\n'\'' alloc perf-inline serde std tz-fat tzdb-bundle-always' \
  "    if [ \"\${MOCK_JIFF_MODE:-valid}\" = \"feature-violation\" ]; then" \
  '      printf '\''jiff feature "%s"\n'\'' tz-system' \
  '    fi' \
  '    ;;' \
  '  *"-p nautilus-pyo3"*)' \
  '    printf '\''jiff feature "%s"\n'\'' alloc default perf-inline serde std tz-fat tz-system tzdb-bundle-always tzdb-bundle-platform tzdb-concatenated tzdb-zoneinfo' \
  '    ;;' \
  '  *)' \
  "    echo \"Unexpected cargo tree package: \$*\" >&2" \
  '    exit 2' \
  '    ;;' \
  'esac' > "$CASE_ROOT/bin/cargo"
chmod +x "$CASE_ROOT/bin/cargo"

create_case() {
  local name="$1"
  local declaration="$2"
  local case_dir="$CASE_ROOT/$name"

  mkdir -p "$case_dir/.pre-commit-hooks" "$case_dir/crates/example" "$case_dir/patches"
  cp "$HOOK" "$case_dir/.pre-commit-hooks/check_jiff_features.sh"
  printf '%s\n' '[dependencies]' "$declaration" > "$case_dir/crates/example/Cargo.toml"

  printf '%s' "$case_dir"
}

run_hook() {
  local case_dir="$1"
  local mode="$2"

  set +e
  PATH="$CASE_ROOT/bin:$PATH" MOCK_JIFF_MODE="$mode" \
    bash "$case_dir/.pre-commit-hooks/check_jiff_features.sh" \
    > "$case_dir/stdout.txt" 2> "$case_dir/stderr.txt"
  RUN_STATUS=$?
  set -e
}

expect_failure() {
  local case_dir="$1"
  local mode="$2"
  local reason="$3"

  run_hook "$case_dir" "$mode"
  if [ "$RUN_STATUS" -ne 1 ]; then
    echo "Expected Jiff feature hook to fail in $case_dir"
    cat "$case_dir/stdout.txt" "$case_dir/stderr.txt"
    exit 1
  fi
  if ! rg -Fq "$reason" "$case_dir/stderr.txt"; then
    echo "Expected Jiff failure reason not found: $reason"
    cat "$case_dir/stderr.txt"
    exit 1
  fi
}

valid_case=$(create_case "valid" 'jiff = { workspace = true }')
run_hook "$valid_case" "valid"
if [ "$RUN_STATUS" -ne 0 ]; then
  echo "Expected Jiff feature hook to pass valid input"
  cat "$valid_case/stdout.txt" "$valid_case/stderr.txt"
  exit 1
fi
rg -Fq "Jiff feature policy checks passed" "$valid_case/stdout.txt"

workspace_case=$(create_case "workspace-violation" 'jiff = { workspace = true, features = ["std"] }')
expect_failure "$workspace_case" "valid" "Workspace crates must inherit Jiff without adding features"

patch_case=$(create_case "patch-violation" 'jiff = { workspace = true }')
expect_failure "$patch_case" "patch-violation" "Maintained patches must keep Jiff independent of system time zone sources"

feature_case=$(create_case "feature-violation" 'jiff = { workspace = true }')
expect_failure "$feature_case" "feature-violation" "Jiff feature policy changed for nautilus-trading"

echo "Jiff feature hook tests passed"
