#!/usr/bin/env bash
set -euo pipefail

if ! command -v rustup &> /dev/null; then
  echo "rustup not found, skipping update"
  exit 0
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TOOLCHAIN="$(bash "${SCRIPT_DIR}/../rust-toolchain.sh")"

echo "Updating Rust toolchain ${TOOLCHAIN}..."

max_attempts="${INSTALL_ATTEMPTS:-5}"

if ! [[ "$max_attempts" =~ ^[0-9]+$ ]] || [ "$max_attempts" -lt 1 ]; then
  echo "INSTALL_ATTEMPTS must be a positive integer" >&2
  exit 1
fi

set +e
success=false
for i in $(seq 1 "$max_attempts"); do
  rustup update --force "$TOOLCHAIN"
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
  echo "All Rust toolchain update retries failed"
  exit 1
fi

echo "Rust toolchain update completed successfully"
