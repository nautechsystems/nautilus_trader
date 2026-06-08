#!/usr/bin/env bash
#
# Round-robin every fuzz target indefinitely, allotting a fixed time slice
# per target before rotating. Bails on the first crash and points at the
# artifact under `artifacts/<target>/`.
#
# Usage:
#   ./grind.sh                 # 300s per target, cycle forever
#   ./grind.sh 600             # 10 min per target
#   ./grind.sh 600 fuzz_verify # only one target, 10 min slices
#
# Corpus accumulates under `corpus/<target>/` and persists across runs;
# longer total wall time keeps growing coverage. Stop with Ctrl-C between
# slices or send SIGINT to libfuzzer to flush stats early.
#
# Resource note: each slice pegs one CPU core. Run only when the machine
# is otherwise idle; the cargo-fuzz process will fight any other heavy
# workload for cache and core time.

set -euo pipefail

cd "$(dirname "$0")"

per_target_secs="${1:-300}"
filter="${2:-}"

if ! command -v cargo-fuzz > /dev/null 2>&1; then
  echo "cargo-fuzz not installed. Run \`make install-tools\` from the repo root." >&2
  exit 1
fi

mapfile -t all_targets < <(cargo +nightly fuzz list)

# Optional second arg is a substring filter against target names. Cycles
# only the matching subset (e.g. `grind.sh 600 pornin` runs every
# `fuzz_pornin_diff_*` target). An exact target name is a substring of
# itself, so single-target invocations from earlier scripts still work.
if [[ -n "$filter" ]]; then
  targets=()
  for t in "${all_targets[@]}"; do
    if [[ "$t" == *"$filter"* ]]; then
      targets+=("$t")
    fi
  done
  if [[ ${#targets[@]} -eq 0 ]]; then
    echo "No fuzz targets matching filter: $filter" >&2
    exit 1
  fi
else
  targets=("${all_targets[@]}")
fi

total=${#targets[@]}
if [[ $total -eq 0 ]]; then
  echo "No fuzz targets registered." >&2
  exit 1
fi

cycle_secs=$((per_target_secs * total))
cycle_mins=$((cycle_secs / 60))
echo "Grinding $total target(s), ${per_target_secs}s per slice."
echo "Full cycle: ~${cycle_mins} min (~${cycle_secs}s). Ctrl-C between slices to stop."
echo "Corpus persists under corpus/<target>/; crashes land in artifacts/<target>/."

cycle=0
while true; do
  cycle=$((cycle + 1))
  echo
  echo "=== Cycle $cycle starting at $(date '+%Y-%m-%d %H:%M:%S') ==="
  for target in "${targets[@]}"; do
    rotates_at=$(date -d "+${per_target_secs} seconds" '+%H:%M:%S' 2> /dev/null ||
      date -v "+${per_target_secs}S" '+%H:%M:%S')
    echo
    echo "--- $target (cycle $cycle, rotates at ${rotates_at}) ---"
    if ! cargo +nightly fuzz run "$target" -- \
      -max_total_time="$per_target_secs" -print_final_stats=1; then
      echo
      echo "!!! $target crashed during cycle $cycle."
      echo "    See artifacts/$target/ for the reproducer."
      exit 1
    fi
  done
done
