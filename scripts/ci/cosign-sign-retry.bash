#!/usr/bin/env bash
# Sign OCI image references with cosign using bounded retry.
set -euo pipefail

attempts="${COSIGN_SIGN_ATTEMPTS:-5}"
retry_delay_seconds="${COSIGN_SIGN_RETRY_DELAY_SECONDS:-15}"

validate_positive_integer() {
  local name=$1
  local value=$2

  if ! [[ "$value" =~ ^[0-9]+$ ]] || [[ "$value" -lt 1 ]]; then
    echo "::error::${name} must be a positive integer."
    exit 1
  fi
}

validate_positive_integer COSIGN_SIGN_ATTEMPTS "$attempts"
validate_positive_integer COSIGN_SIGN_RETRY_DELAY_SECONDS "$retry_delay_seconds"

if [[ "$#" -eq 0 ]]; then
  echo "::error::Usage: cosign-sign-retry.bash <image-ref> [<image-ref>...]"
  exit 1
fi
if ! command -v cosign > /dev/null; then
  echo "::error::cosign not found."
  exit 1
fi

sign_image() {
  local image=$1
  local status=0
  local delay="$retry_delay_seconds"

  for attempt in $(seq 1 "$attempts"); do
    set +e
    cosign sign --yes "$image"
    status=$?
    set -e

    if [[ "$status" -eq 0 ]]; then
      echo "Signed image ${image}."
      return 0
    fi

    if [[ "$attempt" -lt "$attempts" ]]; then
      echo "cosign sign failed for ${image} (exit=${status}), retry (${attempt}/${attempts}) after ${delay}s"
      sleep "$delay"
      delay=$((delay * 2))
    fi
  done

  echo "::error::cosign sign failed for ${image} after ${attempts} attempts."
  return "$status"
}

for image in "$@"; do
  sign_image "$image"
done
