#!/usr/bin/env bash
set -euo pipefail

# Update rustup with retries to handle transient network failures.

if ! command -v rustup &> /dev/null; then
  echo "rustup not found, skipping update"
  exit 0
fi

echo "Updating rustup..."

max_attempts="${INSTALL_ATTEMPTS:-5}"

if ! [[ "$max_attempts" =~ ^[0-9]+$ ]] || [ "$max_attempts" -lt 1 ]; then
  echo "INSTALL_ATTEMPTS must be a positive integer" >&2
  exit 1
fi

set +e
success=false
for i in $(seq 1 "$max_attempts"); do
  rustup update --force
  status=$?
  if [ $status -eq 0 ]; then
    success=true
    break
  else
    echo "rustup update failed (exit=$status), retry ($i/${max_attempts})"
    if [ "$i" -lt "$max_attempts" ]; then
      sleep $((2 ** i))
    fi
  fi
done
set -e

if [ "$success" != "true" ]; then
  echo "All rustup update retries failed"
  exit 1
fi

echo "rustup update completed successfully"
