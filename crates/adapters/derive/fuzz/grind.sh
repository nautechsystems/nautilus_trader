#!/usr/bin/env sh

set -eu

cd "$(dirname "$0")"

per_target_secs="${1:-300}"
filter="${2:-}"

if ! command -v cargo-fuzz > /dev/null 2>&1; then
  echo "cargo-fuzz not installed. Run \`cargo install cargo-fuzz\`." >&2
  exit 1
fi

all_targets="$(cargo +nightly fuzz list)"
if [ -n "$filter" ]; then
  targets="$(printf '%s\n' "$all_targets" | grep -F "$filter" || true)"
else
  targets="$all_targets"
fi

target_count="$(printf '%s\n' "$targets" | sed '/^$/d' | wc -l | tr -d ' ')"
if [ "$target_count" = "0" ]; then
  echo "No fuzz targets registered." >&2
  exit 1
fi

cycle_secs=$((per_target_secs * target_count))
cycle_mins=$((cycle_secs / 60))

echo "Grinding $target_count target(s), ${per_target_secs}s per slice."
echo "Full cycle: ~${cycle_mins} min (~${cycle_secs}s). Ctrl-C between slices to stop."
echo "Corpus persists under corpus/<target>/; crashes land in artifacts/<target>/."

cycle=0
while true; do
  cycle=$((cycle + 1))
  echo
  echo "Cycle $cycle starting at $(date '+%Y-%m-%d %H:%M:%S')"
  for target in $targets; do
    echo
    echo "$target (cycle $cycle)"
    if ! cargo +nightly fuzz run "$target" -- \
      -max_total_time="$per_target_secs" -print_final_stats=1; then
      echo
      echo "$target crashed during cycle $cycle."
      echo "See artifacts/$target/ for the reproducer."
      exit 1
    fi
  done
done
