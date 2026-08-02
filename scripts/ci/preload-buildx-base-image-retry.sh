#!/usr/bin/env bash
# Preload OCI images into the selected Buildx cache with bounded retry.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../.." && pwd)"
platform="${1:?Usage: preload-buildx-base-image-retry.sh <platform> (--from-dockerfile <path> | --image <ref>) [max_attempts]}"
mode="${2:?Usage: preload-buildx-base-image-retry.sh <platform> (--from-dockerfile <path> | --image <ref>) [max_attempts]}"
retry_delay_seconds="${BUILDX_BASE_IMAGE_PULL_RETRY_DELAY_SECONDS:-30}"

validate_positive_integer() {
  local name=$1
  local value=$2

  if ! [[ "$value" =~ ^[0-9]+$ ]] || [[ "$value" -lt 1 ]]; then
    echo "::error::${name} must be a positive integer." >&2
    exit 1
  fi
}

case "$mode" in
  --from-dockerfile)
    dockerfile="${3:?Usage: preload-buildx-base-image-retry.sh <platform> --from-dockerfile <path> [max_attempts]}"
    attempts="${4:-${BUILDX_BASE_IMAGE_PULL_ATTEMPTS:-5}}"
    if [[ "$dockerfile" != /* ]]; then
      dockerfile="${repo_root}/${dockerfile}"
    fi
    if [[ ! -f "$dockerfile" ]]; then
      echo "::error::Dockerfile not found: ${dockerfile}" >&2
      exit 1
    fi
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
    ;;
  --image)
    images=("${3:?Usage: preload-buildx-base-image-retry.sh <platform> --image <ref> [max_attempts]}")
    attempts="${4:-${BUILDX_BASE_IMAGE_PULL_ATTEMPTS:-5}}"
    ;;
  *)
    echo "::error::Unknown preload mode: ${mode}" >&2
    exit 1
    ;;
esac

validate_positive_integer BUILDX_BASE_IMAGE_PULL_ATTEMPTS "$attempts"
validate_positive_integer BUILDX_BASE_IMAGE_PULL_RETRY_DELAY_SECONDS "$retry_delay_seconds"

if [[ "${#images[@]}" -eq 0 ]]; then
  echo "::error::No digest-pinned external base images found." >&2
  exit 1
fi
for image in "${images[@]}"; do
  if ! [[ "$image" =~ @sha256:[0-9a-f]{64}$ ]]; then
    echo "::error::Expected a digest-pinned image reference: ${image}" >&2
    exit 1
  fi
done

context_dir="$(mktemp -d)"
trap 'rm -rf "$context_dir"' EXIT

preload_image() {
  local image=$1
  local delay="$retry_delay_seconds"
  local attempt status

  for attempt in $(seq 1 "$attempts"); do
    status=0
    if docker buildx build \
      --file "${repo_root}/.docker/preload-base-image.dockerfile" \
      --platform "$platform" \
      --output type=cacheonly \
      --build-arg "BASE_IMAGE=${image}" \
      "$context_dir"; then
      return 0
    else
      status=$?
    fi

    if [[ "$attempt" -lt "$attempts" ]]; then
      echo "Buildx preload failed for ${image} (exit=${status}), retry (${attempt}/${attempts}) after ${delay}s"
      sleep "$delay"
      delay=$((delay * 2))
    fi
  done

  echo "::error::Buildx preload failed for ${image} after ${attempts} attempts." >&2
  return "$status"
}

for image in "${images[@]}"; do
  preload_image "$image"
done
