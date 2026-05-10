#!/usr/bin/env bash
set -euo pipefail

# Throttle CARGO_BUILD_JOBS on cold caches to avoid OOM on CI runners.
# Warm cache (*.d files present) uses the cargo default (all CPUs);
# cold cache limits to 2 jobs.

if find "$CARGO_TARGET_DIR" -name '*.d' -print -quit 2> /dev/null | grep -q .; then
  echo "Cache is warm, using default cargo parallelism"
else
  echo "Cold cache detected, limiting to 2 build jobs"
  echo "CARGO_BUILD_JOBS=2" >> "$GITHUB_ENV"
fi
