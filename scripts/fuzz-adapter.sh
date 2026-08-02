#!/usr/bin/env sh

set -eu

if [ "$#" -lt 1 ] || [ "$#" -gt 3 ]; then
  echo "Usage: $0 <adapter> [seconds-per-target] [target-filter]" >&2
  exit 1
fi

adapter="$1"
per_target_secs="${2:-300}"
filter="${3:-}"

case "$adapter" in
  *[!a-z0-9_]* | "")
    echo "Invalid adapter name: $adapter" >&2
    exit 1
    ;;
esac

repo_root="$(CDPATH='' cd "$(dirname "$0")/.." && pwd)"
fuzz_dir="crates/adapters/$adapter"
cd "$repo_root"

# Sanitizer coverage does not link with the workspace's fat-LTO release profile
export CARGO_PROFILE_RELEASE_LTO=false

if [ ! -f "$fuzz_dir/Cargo.toml" ]; then
  echo "Adapter manifest not found: $fuzz_dir/Cargo.toml" >&2
  exit 1
fi

if ! command -v cargo-fuzz > /dev/null 2>&1; then
  echo "cargo-fuzz not installed. Run \`make install-tools\` from the repo root." >&2
  exit 1
fi

all_targets="$(cargo +nightly fuzz list --fuzz-dir "$fuzz_dir" | sed -n '/^fuzz_/p')"
if [ -n "$filter" ]; then
  targets="$(printf '%s\n' "$all_targets" | grep -F "$filter" || true)"
else
  targets="$all_targets"
fi

target_count="$(printf '%s\n' "$targets" | sed '/^$/d' | wc -l | tr -d ' ')"
if [ "$target_count" = "0" ]; then
  echo "No fuzz targets registered for adapter: $adapter" >&2
  exit 1
fi

cycle_secs=$((per_target_secs * target_count))
cycle_mins=$((cycle_secs / 60))

echo "Grinding $target_count $adapter target(s), ${per_target_secs}s per slice."
echo "Full cycle: ~${cycle_mins} min (~${cycle_secs}s). Ctrl-C between slices to stop."
echo "Corpus persists under $fuzz_dir/corpus/<target>/."
echo "Crashes land under $fuzz_dir/artifacts/<target>/."

cycle=0
while true; do
  cycle=$((cycle + 1))
  echo
  echo "Cycle $cycle starting at $(date '+%Y-%m-%d %H:%M:%S')"
  for target in $targets; do
    echo
    echo "$target (cycle $cycle)"
    if ! cargo +nightly fuzz run "$target" \
      --fuzz-dir "$fuzz_dir" \
      --features fuzz \
      -- \
      -max_total_time="$per_target_secs" \
      -print_final_stats=1; then
      echo
      echo "$target crashed during cycle $cycle." >&2
      echo "See $fuzz_dir/artifacts/$target/ for the reproducer." >&2
      exit 1
    fi
  done
done
