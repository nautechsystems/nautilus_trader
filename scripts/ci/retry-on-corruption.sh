#!/usr/bin/env bash
set -uo pipefail

# Retry a command up to MAX_RETRIES times if the failure matches known macOS
# runner filesystem corruption patterns (null bytes in Python sources, corrupt
# build-script binaries, invalid archives, and bad cached files). On a
# corruption match the script resets build artifacts and uses a fresh uv cache
# before retrying.
#
# Usage: retry-on-corruption.sh <command> [args...]

MAX_RETRIES=3

CORRUPTION_PATTERNS=(
  "cannot contain null bytes"
  "cannot execute binary file"
  "Failed to clone"
  "Illegal byte sequence"
  "Metadata field Name not found"
  "archive member invalid control bits"
  "slice is not valid mach-o file"
  "unknown file type in"
)

log_file="$(mktemp)"
temp_uv_cache_dirs=()

# shellcheck disable=SC2317,SC2329
# ShellCheck does not follow EXIT trap callbacks
cleanup_temp_files() {
  rm -f "$log_file"

  if [ "${#temp_uv_cache_dirs[@]}" -gt 0 ]; then
    for uv_cache_dir in "${temp_uv_cache_dirs[@]}"; do
      rm -rf "$uv_cache_dir" 2> /dev/null || true
    done
  fi
}

trap cleanup_temp_files EXIT

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

  if command -v uv > /dev/null 2>&1; then
    reset_uv_cache
  fi

  if { [ "${1:-}" != "uv" ] || [ "${2:-}" != "pip" ]; } && [ -d ".venv" ]; then
    echo "Removing virtual environment: .venv"
    rm -rf .venv
  fi
}

reset_uv_cache() {
  previous_uv_cache_dir="${UV_CACHE_DIR:-}"
  uv_cache_parent="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
  fresh_uv_cache_dir="$(mktemp -d "$uv_cache_parent/uv-cache-retry.XXXXXX")"

  temp_uv_cache_dirs+=("$fresh_uv_cache_dir")
  export UV_CACHE_DIR="$fresh_uv_cache_dir"
  echo "Using fresh uv cache: $UV_CACHE_DIR"

  if [ -n "$previous_uv_cache_dir" ] &&
    [ "$previous_uv_cache_dir" != "$UV_CACHE_DIR" ] &&
    [ "$previous_uv_cache_dir" != "/" ] &&
    [ "$previous_uv_cache_dir" != "." ]; then
    echo "Removing previous uv cache: $previous_uv_cache_dir"
    rm -rf "$previous_uv_cache_dir" 2> /dev/null || true
  fi
}

for attempt in $(seq 0 "$MAX_RETRIES"); do
  if [ "$attempt" -gt 0 ]; then
    echo "Retry $attempt/$MAX_RETRIES..."
    clean_artifacts "$@"
  fi

  if run_and_check "$@"; then
    exit 0
  fi
done

echo "All $MAX_RETRIES retries exhausted"
exit 1
