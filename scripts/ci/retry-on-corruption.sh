#!/usr/bin/env bash
set -uo pipefail

# Retry a command up to MAX_RETRIES times if the failure matches known macOS
# runner filesystem corruption patterns (null bytes in Python sources, corrupt
# build-script binaries). On a corruption match the script purges the Cargo
# target dir and UV build cache before retrying.
#
# Usage: retry-on-corruption.sh <command> [args...]

MAX_RETRIES=3

CORRUPTION_PATTERNS=(
  "cannot contain null bytes"
  "cannot execute binary file"
  "Metadata field Name not found"
)

log_file="$(mktemp)"
trap 'rm -f "$log_file"' EXIT

run_and_check() {
  echo "Running: $*"
  "$@" 2>&1 | tee "$log_file"
  rc=${PIPESTATUS[0]}

  if [ "$rc" -eq 0 ]; then
    return 0
  fi

  # Check if the failure matches a known corruption pattern
  for pattern in "${CORRUPTION_PATTERNS[@]}"; do
    if grep -qF "$pattern" "$log_file"; then
      echo "::warning::Detected runner corruption: '$pattern'"
      return 1
    fi
  done

  echo "Build failed with exit code $rc (not a corruption pattern)"
  exit "$rc"
}

clean_artifacts() {
  echo "Cleaning build artifacts before retry..."

  if [ -n "${CARGO_TARGET_DIR:-}" ] && [ -d "$CARGO_TARGET_DIR" ]; then
    echo "Removing CARGO_TARGET_DIR: $CARGO_TARGET_DIR"
    rm -rf "$CARGO_TARGET_DIR"
  fi

  # Prune the full uv cache (wheels, archives, and builds) so corrupted
  # metadata from installed packages or cached wheels is not reused.
  if command -v uv > /dev/null 2>&1; then
    echo "Pruning uv cache"
    uv cache prune
  fi
}

for attempt in $(seq 0 "$MAX_RETRIES"); do
  if [ "$attempt" -gt 0 ]; then
    echo "Retry $attempt/$MAX_RETRIES..."
    clean_artifacts
  fi

  if run_and_check "$@"; then
    exit 0
  fi
done

echo "All $MAX_RETRIES retries exhausted"
exit 1
