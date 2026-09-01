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

if run_selection "$case_root/missing" failure "" failure "" failure ""; then
  echo "Expected failed attempts to produce no bundle" >&2
  exit 1
fi

if run_selection "$case_root/empty" success "" success later.bundle failure ""; then
  echo "Expected an empty successful bundle path to fail" >&2
  exit 1
fi

if run_selection "$case_root/arguments" success only-two; then
  echo "Expected an invalid argument count to fail" >&2
  exit 1
fi

echo "Attestation bundle selection tests passed"
