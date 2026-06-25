#!/usr/bin/env bash
# Verify GitHub artifact or OCI attestations with bounded retry.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/ci/release-verification-retry.bash
source "${script_dir}/release-verification-retry.bash"

attempts="${GH_ATTESTATION_VERIFY_ATTEMPTS:-7}"
retry_delay_seconds="${GH_ATTESTATION_VERIFY_RETRY_DELAY_SECONDS:-15}"
max_retry_delay_seconds="${GH_ATTESTATION_VERIFY_MAX_RETRY_DELAY_SECONDS:-120}"
command_timeout_seconds="${GH_ATTESTATION_VERIFY_COMMAND_TIMEOUT_SECONDS:-0}"
github_server_url="${GITHUB_SERVER_URL:-https://github.com}"
attestation_identity="${ATTESTATION_IDENTITY:-}"
attestation_issuer="${ATTESTATION_ISSUER:-https://token.actions.githubusercontent.com}"
predicate_type="${ATTESTATION_PREDICATE_TYPE:-}"

validate_positive_integer() {
  local name=$1
  local value=$2

  if ! [[ "$value" =~ ^[0-9]+$ ]] || [[ "$value" -lt 1 ]]; then
    echo "::error::${name} must be a positive integer."
    exit 1
  fi
}

validate_positive_integer GH_ATTESTATION_VERIFY_ATTEMPTS "$attempts"
validate_positive_integer GH_ATTESTATION_VERIFY_RETRY_DELAY_SECONDS "$retry_delay_seconds"
validate_positive_integer GH_ATTESTATION_VERIFY_MAX_RETRY_DELAY_SECONDS "$max_retry_delay_seconds"
if ! [[ "$command_timeout_seconds" =~ ^[0-9]+$ ]]; then
  echo "::error::GH_ATTESTATION_VERIFY_COMMAND_TIMEOUT_SECONDS must be a non-negative integer."
  exit 1
fi

if [[ "$#" -eq 0 ]]; then
  echo "::error::Usage: verify-gh-attestations.bash <subject> [<subject>...]"
  exit 1
fi
if [[ -z "${GITHUB_REPOSITORY:-}" ]]; then
  echo "::error::GITHUB_REPOSITORY not set."
  exit 1
fi
if [[ -z "${GITHUB_TOKEN:-}" && -z "${GH_TOKEN:-}" ]]; then
  echo "::error::GITHUB_TOKEN or GH_TOKEN not set."
  exit 1
fi
if [[ -z "$attestation_identity" && -n "${GITHUB_WORKFLOW_REF:-}" ]]; then
  attestation_identity="${github_server_url}/${GITHUB_WORKFLOW_REF}"
fi
if [[ -z "$attestation_identity" ]]; then
  echo "::error::ATTESTATION_IDENTITY or GITHUB_WORKFLOW_REF is required."
  exit 1
fi
if ! command -v gh > /dev/null; then
  echo "::error::gh not found."
  exit 1
fi
if ! gh attestation --help > /dev/null 2>&1; then
  echo "::error::This GitHub CLI version does not support 'gh attestation'."
  exit 1
fi

subjects=()
for subject in "$@"; do
  case "$subject" in
    oci://*) ;;
    *)
      if [[ ! -f "$subject" ]]; then
        echo "::error::Attestation subject not found: $subject"
        exit 1
      fi
      ;;
  esac
  subjects+=("$subject")
done

run_with_timeout() {
  if [[ "$command_timeout_seconds" -eq 0 ]] || ! command -v timeout > /dev/null; then
    "$@"
  else
    timeout "$command_timeout_seconds" "$@"
  fi
}

verify_subject() {
  local subject=$1
  local command=(
    gh attestation verify "$subject"
    --repo "$GITHUB_REPOSITORY"
    --cert-identity "$attestation_identity"
    --cert-oidc-issuer "$attestation_issuer"
  )

  if [[ -n "$predicate_type" ]]; then
    command+=(--predicate-type "$predicate_type")
  fi

  run_release_verification_with_retry \
    "gh attestation verify ${subject}" \
    "$attempts" \
    "$retry_delay_seconds" \
    "$max_retry_delay_seconds" \
    run_with_timeout "${command[@]}"
  echo "Verified attestation for ${subject}."
}

for subject in "${subjects[@]}"; do
  verify_subject "$subject"
done
