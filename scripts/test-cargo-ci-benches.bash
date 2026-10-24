#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
case_root=$(mktemp -d "${TMPDIR:-/tmp}/nautilus-ci-benches.XXXXXX")
trap 'rm -rf "$case_root"' EXIT
mkdir "$case_root/bin"

cat > "$case_root/bin/cargo" << 'CARGO'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "${CARGO_LOG:?}"
if [[ "$4" == "${FAIL_CRATE:?}" ]]; then
  echo "Mock benchmark failure for $4" >&2
  exit 17
fi
CARGO
chmod +x "$case_root/bin/cargo"

run_case() {
  local fail_crate="$1"
  local expected_status="$2"
  local expected_crates="$3"
  local status=0
  local crate

  : > "$case_root/cargo.log"
  PATH="$case_root/bin:$PATH" CARGO_LOG="$case_root/cargo.log" FAIL_CRATE="$fail_crate" \
    make -C "$repo_root" --no-print-directory -o check-cargo-cooldown \
    CI_BENCH_CRATES='nautilus-core nautilus-model nautilus-common' \
    cargo-ci-benches > "$case_root/output" 2>&1 || status=$?

  if [[ "$status" -ne "$expected_status" ]]; then
    echo "Expected make status $expected_status for $fail_crate, was $status" >&2
    cat "$case_root/output" >&2
    exit 1
  fi

  : > "$case_root/expected.log"
  for crate in $expected_crates; do
    printf 'bench --locked -p %s --profile bench --benches --no-fail-fast\n' "$crate" \
      >> "$case_root/expected.log"
  done
  diff -u "$case_root/expected.log" "$case_root/cargo.log"

  if [[ "$expected_status" -ne 0 ]]; then
    grep -Fq "Mock benchmark failure for $fail_crate" "$case_root/output"
  fi
}

run_case nautilus-core 2 'nautilus-core'
run_case nautilus-model 2 'nautilus-core nautilus-model'
run_case nautilus-common 2 'nautilus-core nautilus-model nautilus-common'
run_case none 0 'nautilus-core nautilus-model nautilus-common'

echo 'Cargo CI benchmark script tests passed'
