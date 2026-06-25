#!/usr/bin/env bash
# Retry release verification commands only for transient verifier lag.
set -euo pipefail

release_verification_validate_positive_integer() {
  local name=$1
  local value=$2

  if ! [[ "$value" =~ ^[0-9]+$ ]] || [[ "$value" -lt 1 ]]; then
    echo "::error::${name} must be a positive integer."
    return 1
  fi
}

release_verification_failure_is_fail_fast() {
  local output_file=$1
  local mismatch_terms
  local identity_terms
  local exact_mismatch_terms

  mismatch_terms="mismatch|did not match|does not match|unexpected|no matching|not trusted|wrong"
  identity_terms="provenance|publisher|repository|workflow|environment|cert-identity|certificate identity|issuer|predicate"
  exact_mismatch_terms="subject.*([[:space:]-]mismatch|did not match|does not match|differs)"
  exact_mismatch_terms="${exact_mismatch_terms}|artifact.*(digest|sha256|hash|checksum).*([[:space:]-]mismatch|did not match|does not match|differs)"
  exact_mismatch_terms="${exact_mismatch_terms}|(digest|sha256|hash|checksum).*([[:space:]-]mismatch|did not match|does not match|differs).*(artifact|bundle)"

  grep -Eiq "(${identity_terms}).*(${mismatch_terms})|(${mismatch_terms}).*(${identity_terms})|${exact_mismatch_terms}" "$output_file"
}

release_verification_failure_is_retryable() {
  local output_file=$1
  local retryable_terms

  if release_verification_failure_is_fail_fast "$output_file"; then
    return 1
  fi

  retryable_terms="invalid log entry|checkpoint|signature[[:space:]]+not[[:space:]]+found|rekor|tuf|sigstore|consisten|transparency[[:space:]]+log|inclusion"
  retryable_terms="${retryable_terms}|timeout|timed out|connection reset|connection refused|temporarily unavailable|too many requests|rate limit"
  retryable_terms="${retryable_terms}|failed to fetch|could not fetch|service unavailable|internal server error|HTTP[[:space:]]+(404|429|5[0-9][0-9])|status[[:space:]]+(404|429|5[0-9][0-9])|returned error:[[:space:]]+404"
  grep -Eiq "$retryable_terms" "$output_file"
}

run_release_verification_with_retry() {
  local description=$1
  local attempts=$2
  local initial_delay_seconds=$3
  local max_delay_seconds=$4
  shift 4

  release_verification_validate_positive_integer RELEASE_VERIFICATION_ATTEMPTS "$attempts"
  release_verification_validate_positive_integer RELEASE_VERIFICATION_RETRY_DELAY_SECONDS "$initial_delay_seconds"
  release_verification_validate_positive_integer RELEASE_VERIFICATION_MAX_RETRY_DELAY_SECONDS "$max_delay_seconds"

  local output_file
  output_file="$(mktemp "${TMPDIR:-/tmp}/release-verification.XXXXXX")"

  local attempt delay_seconds status
  delay_seconds="$initial_delay_seconds"
  status=0

  for attempt in $(seq 1 "$attempts"); do
    : > "$output_file"

    if "$@" > "$output_file" 2>&1; then
      status=0
    else
      status=$?
    fi

    if [[ "$status" -eq 0 ]]; then
      cat "$output_file"
      rm -f "$output_file"
      return 0
    fi

    if ! release_verification_failure_is_retryable "$output_file"; then
      cat "$output_file" >&2
      echo "::error::${description} failed with a non-retryable verification error (exit=${status})." >&2
      rm -f "$output_file"
      return "$status"
    fi

    if [[ "$attempt" -lt "$attempts" ]]; then
      cat "$output_file" >&2
      echo "${description} failed with a retryable verification error (exit=${status}), retry (${attempt}/${attempts}) after ${delay_seconds}s" >&2
      "${RELEASE_VERIFICATION_SLEEP_COMMAND:-sleep}" "$delay_seconds"
      delay_seconds=$((delay_seconds * 2))
      if [[ "$delay_seconds" -gt "$max_delay_seconds" ]]; then
        delay_seconds="$max_delay_seconds"
      fi
    fi
  done

  cat "$output_file" >&2
  echo "::error::${description} failed after ${attempts} retryable attempts." >&2
  rm -f "$output_file"
  return "$status"
}
