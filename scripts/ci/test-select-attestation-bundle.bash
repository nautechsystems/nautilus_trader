#!/usr/bin/env bash
set -euo pipefail

repo_root=$(git rev-parse --show-toplevel)
script="$repo_root/scripts/ci/select-attestation-bundle.bash"
case_root=$(mktemp -d)
trap 'rm -rf "$case_root"' EXIT

run_selection() {
  output=$1
  shift
  GITHUB_OUTPUT="$output" bash "$script" "$@"
}

output="$case_root/first"
run_selection "$output" success first.bundle failure "" failure ""
grep -Fxq 'bundle-path=first.bundle' "$output"

output="$case_root/second"
run_selection "$output" failure "" success second.bundle failure ""
grep -Fxq 'bundle-path=second.bundle' "$output"

status=0
diagnostic=$(run_selection "$case_root/missing" failure "" failure "" failure "" 2>&1) || status=$?
expected='::error::No build provenance bundle path was produced'
if [ "$status" -ne 1 ] || [ "$diagnostic" != "$expected" ]; then
  printf 'Expected status 1 and diagnostic: %s\n' "$expected" >&2
  printf 'Actual status %s and diagnostic: %s\n' "$status" "$diagnostic" >&2
  exit 1
fi

status=0
diagnostic=$(run_selection "$case_root/empty" success "" success later.bundle failure "" 2>&1) || status=$?
expected='::error::A successful attestation produced no bundle path'
if [ "$status" -ne 1 ] || [ "$diagnostic" != "$expected" ]; then
  printf 'Expected status 1 and diagnostic: %s\n' "$expected" >&2
  printf 'Actual status %s and diagnostic: %s\n' "$status" "$diagnostic" >&2
  exit 1
fi

status=0
diagnostic=$(run_selection "$case_root/arguments" success only-two 2>&1) || status=$?
expected='Usage: select-attestation-bundle.bash OUTCOME PATH OUTCOME PATH OUTCOME PATH'
if [ "$status" -ne 1 ] || [ "$diagnostic" != "$expected" ]; then
  printf 'Expected status 1 and diagnostic: %s\n' "$expected" >&2
  printf 'Actual status %s and diagnostic: %s\n' "$status" "$diagnostic" >&2
  exit 1
fi

echo "Attestation bundle selection tests passed"
