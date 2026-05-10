#!/usr/bin/env bash
# Returns a comma-separated list of all features for a crate, excluding
# "extension-module" and "default" so that test builds work without a
# Python interpreter linked.
#
# Usage: scripts/crate-test-features.sh <crate-name>
# Example: scripts/crate-test-features.sh nautilus-live
#   -> python,ffi,streaming,defi

set -euo pipefail

if [ $# -ne 1 ]; then
  echo "Usage: $0 <crate-name>" >&2
  exit 1
fi

cargo metadata --no-deps --format-version 1 |
  jq -r --arg p "$1" '
        [.packages[]
         | select(.name == $p)
         | .features
         | keys[]
         | select(. != "extension-module" and . != "default")
        ] | join(",")'
