#!/usr/bin/env bash
set -euo pipefail

if (($# != 6)); then
  echo "Usage: select-attestation-bundle.bash OUTCOME PATH OUTCOME PATH OUTCOME PATH" >&2
  exit 1
fi

while (($# > 0)); do
  outcome=$1
  bundle_path=$2
  shift 2

  if [ "$outcome" = "success" ]; then
    if [ -z "$bundle_path" ]; then
      echo "::error::A successful attestation produced no bundle path" >&2
      exit 1
    fi
    printf 'bundle-path=%s\n' "$bundle_path" >> "${GITHUB_OUTPUT:?GITHUB_OUTPUT is required}"
    exit 0
  fi
done

echo "::error::No build provenance bundle path was produced" >&2
exit 1
