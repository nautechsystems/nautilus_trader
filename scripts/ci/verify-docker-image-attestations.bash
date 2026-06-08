#!/usr/bin/env bash
# Verify an OCI image cosign signature and SPDX SBOM attestation with bounded retry.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"

attempts="${DOCKER_VERIFY_ATTEMPTS:-3}"
retry_delay_seconds="${DOCKER_VERIFY_RETRY_DELAY_SECONDS:-30}"
command_timeout_seconds="${DOCKER_VERIFY_COMMAND_TIMEOUT_SECONDS:-90}"
cosign_timeout="${COSIGN_VERIFY_TIMEOUT:-60s}"
github_server_url="${GITHUB_SERVER_URL:-https://github.com}"
attestation_identity="${ATTESTATION_IDENTITY:-}"
attestation_issuer="${ATTESTATION_ISSUER:-https://token.actions.githubusercontent.com}"
predicate_type="${SPDX_PREDICATE_TYPE:-https://spdx.dev/Document/v2.3}"

validate_positive_integer() {
  local name=$1
  local value=$2

  if ! [[ "$value" =~ ^[0-9]+$ ]] || [[ "$value" -lt 1 ]]; then
    echo "::error::${name} must be a positive integer."
    exit 1
  fi
}

validate_positive_integer DOCKER_VERIFY_ATTEMPTS "$attempts"
validate_positive_integer DOCKER_VERIFY_RETRY_DELAY_SECONDS "$retry_delay_seconds"
validate_positive_integer DOCKER_VERIFY_COMMAND_TIMEOUT_SECONDS "$command_timeout_seconds"

if [[ "$#" -ne 1 ]]; then
  echo "::error::Usage: verify-docker-image-attestations.bash <image-ref>"
  exit 1
fi
if [[ -z "$attestation_identity" && -n "${GITHUB_WORKFLOW_REF:-}" ]]; then
  attestation_identity="${github_server_url}/${GITHUB_WORKFLOW_REF}"
fi
if [[ -z "$attestation_identity" ]]; then
  echo "::error::ATTESTATION_IDENTITY or GITHUB_WORKFLOW_REF is required."
  exit 1
fi
if ! command -v cosign > /dev/null; then
  echo "::error::cosign not found."
  exit 1
fi

image=$1

run_with_timeout() {
  if command -v timeout > /dev/null; then
    timeout "$command_timeout_seconds" "$@"
  else
    "$@"
  fi
}

retry_command() {
  local description=$1
  shift

  local status=0
  local delay="$retry_delay_seconds"

  for attempt in $(seq 1 "$attempts"); do
    set +e
    "$@"
    status=$?
    set -e

    if [[ "$status" -eq 0 ]]; then
      return 0
    fi

    if [[ "$attempt" -lt "$attempts" ]]; then
      echo "${description} failed (exit=${status}), retry (${attempt}/${attempts}) after ${delay}s"
      sleep "$delay"
      delay=$((delay * 2))
    fi
  done

  echo "::error::${description} failed after ${attempts} attempts."
  return "$status"
}

retry_command "cosign verify" \
  run_with_timeout cosign verify "$image" \
  --timeout="$cosign_timeout" \
  --certificate-identity "$attestation_identity" \
  --certificate-oidc-issuer "$attestation_issuer"

ATTESTATION_IDENTITY="$attestation_identity" \
  ATTESTATION_ISSUER="$attestation_issuer" \
  ATTESTATION_PREDICATE_TYPE="$predicate_type" \
  GH_ATTESTATION_VERIFY_ATTEMPTS="$attempts" \
  GH_ATTESTATION_VERIFY_RETRY_DELAY_SECONDS="$retry_delay_seconds" \
  GH_ATTESTATION_VERIFY_COMMAND_TIMEOUT_SECONDS="$command_timeout_seconds" \
  bash "${script_dir}/verify-gh-attestations.bash" "oci://${image}"
