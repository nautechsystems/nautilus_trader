#!/usr/bin/env bash
set -euo pipefail

# Set CARGO_BUILD_JOBS based on whether the Rust cache has compiled artifacts.
# Warm cache (*.d files present) gets 2 jobs; cold cache gets 1 to avoid OOM.

if find "$CARGO_TARGET_DIR" -name '*.d' -print -quit 2> /dev/null | grep -q .; then
  echo "CARGO_BUILD_JOBS=2" >> "$GITHUB_ENV"
else
  echo "CARGO_BUILD_JOBS=1" >> "$GITHUB_ENV"
fi
