#!/usr/bin/env bash
# Pull OCI images with bounded retry.
# Usage: docker-pull-retry.sh <image> [max_attempts]
#        docker-pull-retry.sh --from-dockerfile <path> [max_attempts]

set -euo pipefail

mode="${1:?Usage: docker-pull-retry.sh <image> [max_attempts]}"
retry_delay_seconds="${DOCKER_PULL_RETRY_DELAY_SECONDS:-30}"

validate_positive_integer() {
  local name=$1
  local value=$2

  if ! [[ "$value" =~ ^[0-9]+$ ]] || [[ "$value" -lt 1 ]]; then
    echo "::error::${name} must be a positive integer." >&2
    exit 1
  fi
}

pull_image() {
  local image=$1
  local attempts=$2
  local delay="$retry_delay_seconds"
  local attempt status

  for attempt in $(seq 1 "$attempts"); do
    status=0
    if docker pull "$image"; then
      return 0
    else
      status=$?
    fi

    if [[ "$attempt" -lt "$attempts" ]]; then
      echo "docker pull failed for ${image} (exit=${status}), retry (${attempt}/${attempts}) after ${delay}s"
      sleep "$delay"
      delay=$((delay * 2))
    fi
  done

  echo "::error::docker pull failed for ${image} after ${attempts} attempts." >&2
  return "$status"
}

if [[ "$mode" == "--from-dockerfile" ]]; then
  dockerfile="${2:?Usage: docker-pull-retry.sh --from-dockerfile <path> [max_attempts]}"
  attempts="${3:-${DOCKER_PULL_ATTEMPTS:-5}}"
  images=()
  while IFS= read -r image; do
    images+=("$image")
  done < <(
    awk '
      $1 == "FROM" && $2 ~ /@sha256:/ && $2 !~ /\$/ { print $2 }
      $1 == "COPY" && $2 ~ /^--from=.*@sha256:/ {
        sub(/^--from=/, "", $2)
        print $2
      }
    ' "$dockerfile"
  )
else
  images=("$mode")
  attempts="${2:-${DOCKER_PULL_ATTEMPTS:-5}}"
fi

validate_positive_integer DOCKER_PULL_ATTEMPTS "$attempts"
validate_positive_integer DOCKER_PULL_RETRY_DELAY_SECONDS "$retry_delay_seconds"

if [[ "${#images[@]}" -eq 0 ]]; then
  echo "::error::No digest-pinned external base images found." >&2
  exit 1
fi

for image in "${images[@]}"; do
  pull_image "$image" "$attempts"
done
