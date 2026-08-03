#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 <file>" >&2
  exit 1
fi

if command -v sha256sum > /dev/null 2>&1; then
  sha256sum "$1" | awk '{ print $1 }'
elif command -v shasum > /dev/null 2>&1; then
  shasum -a 256 "$1" | awk '{ print $1 }'
else
  echo "Error: sha256sum or shasum is required" >&2
  exit 1
fi
