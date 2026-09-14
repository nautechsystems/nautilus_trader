#!/usr/bin/env bash
set -euo pipefail

# Print the base Cargo feature set shared by the lint, doc, and test gates.
#
# The Makefile, the changed-crate clippy and doc hooks, and the script tests that
# pin their commands all read this one definition. Separate copies are how the
# doc hook ended up resolving a different feature graph to everything else, which
# cost it a private set of dependency artifacts.
#
# Usage: cargo-features.bash [--no-defi]
# Example: cargo-features.bash  ->  arrow,ffi,python,high-precision,streaming,defi

INCLUDE_DEFI=true

if (($# > 1)); then
  echo "Usage: cargo-features.bash [--no-defi]" >&2
  exit 2
fi

if (($# == 1)); then
  if [[ "$1" != "--no-defi" ]]; then
    echo "Error: Unknown argument: $1" >&2
    echo "Usage: cargo-features.bash [--no-defi]" >&2
    exit 2
  fi
  INCLUDE_DEFI=false
fi

FEATURES=(arrow ffi python high-precision streaming)

if [[ "$INCLUDE_DEFI" == true ]]; then
  FEATURES+=(defi)
fi

(
  IFS=,
  printf '%s' "${FEATURES[*]}"
)
