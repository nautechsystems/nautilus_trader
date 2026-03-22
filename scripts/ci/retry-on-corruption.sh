#!/usr/bin/env bash
set -uo pipefail

# Retry a command once if the failure matches known macOS runner filesystem
# corruption patterns (null bytes in Python sources, corrupt build-script
# binaries). On a corruption match the script purges the Cargo target dir
# and UV build cache before retrying.
#
# Usage: retry-on-corruption.sh <command> [args...]

CORRUPTION_PATTERNS=(
  "cannot contain null bytes"
  "cannot execute binary file"
)

log_file="$(mktemp)"
trap 'rm -f "$log_file"' EXIT

echo "Running: $*"
"$@" 2>&1 | tee "$log_file"
rc=${PIPESTATUS[0]}

if [ "$rc" -eq 0 ]; then
  exit 0
fi

# Check if the failure matches a known corruption pattern
matched=false
for pattern in "${CORRUPTION_PATTERNS[@]}"; do
  if grep -qF "$pattern" "$log_file"; then
    echo "::warning::Detected runner corruption: '$pattern'"
    matched=true
    break
  fi
done

if [ "$matched" = false ]; then
  echo "Build failed with exit code $rc (not a corruption pattern)"
  exit "$rc"
fi

echo "Cleaning build artifacts before retry..."

# Purge Cargo target dir if set
if [ -n "${CARGO_TARGET_DIR:-}" ] && [ -d "$CARGO_TARGET_DIR" ]; then
  echo "Removing CARGO_TARGET_DIR: $CARGO_TARGET_DIR"
  rm -rf "$CARGO_TARGET_DIR"
fi

# Purge UV build cache (temporary wheel build environments)
if [ -n "${UV_CACHE_DIR:-}" ] && [ -d "$UV_CACHE_DIR/builds-v0" ]; then
  echo "Removing UV build cache: $UV_CACHE_DIR/builds-v0"
  rm -rf "$UV_CACHE_DIR/builds-v0"
fi

echo "Retrying: $*"
"$@"
