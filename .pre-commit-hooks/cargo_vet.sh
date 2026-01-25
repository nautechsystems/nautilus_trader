#!/usr/bin/env bash
# Runs cargo-vet supply chain audit if installed

set -euo pipefail

# Exit cleanly if cargo-vet is not installed
if ! command -v cargo-vet &> /dev/null && ! cargo vet --version &> /dev/null 2>&1; then
  echo "INFO: cargo-vet not installed, skipping supply chain audit"
  exit 0
fi

echo "Running cargo vet..."
cargo vet
