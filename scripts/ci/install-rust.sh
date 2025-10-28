#!/usr/bin/env bash
set -euo pipefail

# Update rustup with retries to handle transient network failures.

if ! command -v rustup &> /dev/null; then
  echo "rustup not found, skipping update"
  exit 0
fi

echo "Updating rustup..."

set +e
success=false
for i in {1..3}; do
  rustup update --force
  status=$?
  if [ $status -eq 0 ]; then
    success=true
    break
  else
    echo "rustup update failed (exit=$status), retry ($i/3)"
    sleep $((2 ** i))
  fi
done
set -e

if [ "$success" != "true" ]; then
  echo "All rustup update retries failed"
  exit 1
fi

echo "rustup update completed successfully"
