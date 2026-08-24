#!/usr/bin/env bash
set -euo pipefail

image=${1:?Usage: save-docker-digest.bash IMAGE DIGEST}
digest=${2:?Usage: save-docker-digest.bash IMAGE DIGEST}
runner_temp=${RUNNER_TEMP:?RUNNER_TEMP is required}

if [[ ! "$image" =~ ^[a-z0-9][a-z0-9_-]*$ ]]; then
  echo "::error::Invalid image artifact name: $image" >&2
  exit 1
fi
if [[ ! "$digest" =~ ^sha256:[0-9a-f]{64}$ ]]; then
  echo "::error::Invalid image digest: $digest" >&2
  exit 1
fi

digest_dir="$runner_temp/digests/$image"
mkdir -p "$digest_dir"
touch "$digest_dir/${digest#sha256:}"
