#!/usr/bin/env bash
# Test the release verification retry classifier with mock verifier commands.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/ci/release-verification-retry.bash
source "${script_dir}/release-verification-retry.bash"

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

export RELEASE_VERIFICATION_SLEEP_COMMAND=:
attempt_file=""

fail() {
  echo "::error::$1" >&2
  exit 1
}

set_attempt_count() {
  printf '%s\n' "$1" > "${attempt_file:?}"
}

attempt_count() {
  cat "${attempt_file:?}"
}

increment_attempt_count() {
  local count

  count="$(attempt_count)"
  count=$((count + 1))
  set_attempt_count "$count"
  printf '%s\n' "$count"
}

transient_checkpoint_then_success() {
  local count

  count="$(increment_attempt_count)"
  if [[ "$count" -lt 3 ]]; then
    echo "invalid log entry: checkpoint is not consistent yet" >&2
    return 17
  fi

  echo "verification succeeded"
}

provenance_mismatch() {
  increment_attempt_count > /dev/null
  echo "provenance mismatch: repository did not match expected workflow" >&2
  return 42
}

tuf_lag() {
  increment_attempt_count > /dev/null
  echo "TUF metadata is not consistent yet" >&2
  return 9
}

transport_failure_then_success() {
  local count

  count="$(increment_attempt_count)"
  if [[ "$count" -lt 2 ]]; then
    echo "failed to fetch attestations: HTTP 503" >&2
    return 12
  fi

  echo "transport retry succeeded"
}

mixed_rekor_digest_then_success() {
  local count

  count="$(increment_attempt_count)"
  if [[ "$count" -lt 2 ]]; then
    echo "Rekor returned no matching log entry for digest sha256:abc" >&2
    return 19
  fi

  echo "mixed retry succeeded"
}

digest_mismatch_with_rekor() {
  increment_attempt_count > /dev/null
  echo "Rekor: artifact digest does not match bundle sha256:aaa != sha256:bbb" >&2
  return 14
}

checkpoint_root_hash_then_success() {
  local count

  count="$(increment_attempt_count)"
  if [[ "$count" -lt 2 ]]; then
    echo "inclusion proof failed: computed root hash does not match the signed checkpoint" >&2
    return 15
  fi

  echo "checkpoint retry succeeded"
}

retryable_until_fourth_attempt() {
  local count

  count="$(increment_attempt_count)"
  if [[ "$count" -lt 4 ]]; then
    echo "checkpoint is not consistent yet" >&2
    return 16
  fi

  echo "backoff retry succeeded"
}

sleep_log=""
capture_sleep_delay() {
  printf '%s\n' "$1" >> "${sleep_log:?}"
}

unknown_verifier_error() {
  increment_attempt_count > /dev/null
  echo "local policy rejected this artifact" >&2
  return 13
}

attempt_file="${work_dir}/transient-count"
set_attempt_count 0
transient_output="${work_dir}/transient-output.txt"
run_release_verification_with_retry \
  "mock PyPI verification" \
  4 \
  1 \
  1 \
  transient_checkpoint_then_success > "$transient_output" 2>&1

if [[ "$(attempt_count)" != "3" ]]; then
  fail "transient checkpoint case should verify on the third attempt."
fi
if ! grep -q "verification succeeded" "$transient_output"; then
  fail "transient checkpoint case did not print the verifier success output."
fi

attempt_file="${work_dir}/provenance-count"
set_attempt_count 0
provenance_output="${work_dir}/provenance-output.txt"
set +e
run_release_verification_with_retry \
  "mock PyPI verification" \
  4 \
  1 \
  1 \
  provenance_mismatch > "$provenance_output" 2>&1
provenance_status=$?
set -e

if [[ "$provenance_status" -eq 0 ]]; then
  fail "provenance mismatch case should fail."
fi
if [[ "$(attempt_count)" != "1" ]]; then
  fail "provenance mismatch case should fail fast after one attempt."
fi
if ! grep -q "non-retryable verification error" "$provenance_output"; then
  fail "provenance mismatch case did not report a non-retryable error."
fi

attempt_file="${work_dir}/tuf-count"
set_attempt_count 0
tuf_output="${work_dir}/tuf-output.txt"
set +e
run_release_verification_with_retry \
  "mock PyPI verification" \
  2 \
  1 \
  1 \
  tuf_lag > "$tuf_output" 2>&1
tuf_status=$?
set -e

if [[ "$tuf_status" -eq 0 ]]; then
  fail "TUF lag case should fail after bounded retry attempts."
fi
if [[ "$(attempt_count)" != "2" ]]; then
  fail "TUF lag case should use exactly two attempts."
fi
if ! grep -q "failed after 2 retryable attempts" "$tuf_output"; then
  fail "TUF lag case did not report bounded retry exhaustion."
fi

attempt_file="${work_dir}/transport-count"
set_attempt_count 0
transport_output="${work_dir}/transport-output.txt"
run_release_verification_with_retry \
  "mock GitHub attestation verification" \
  3 \
  1 \
  1 \
  transport_failure_then_success > "$transport_output" 2>&1

if [[ "$(attempt_count)" != "2" ]]; then
  fail "transport failure case should verify on the second attempt."
fi
if ! grep -q "transport retry succeeded" "$transport_output"; then
  fail "transport failure case did not print the verifier success output."
fi

attempt_file="${work_dir}/mixed-count"
set_attempt_count 0
mixed_output="${work_dir}/mixed-output.txt"
run_release_verification_with_retry \
  "mock Rekor verification" \
  3 \
  1 \
  1 \
  mixed_rekor_digest_then_success > "$mixed_output" 2>&1

if [[ "$(attempt_count)" != "2" ]]; then
  fail "mixed Rekor digest case should verify on the second attempt."
fi
if ! grep -q "mixed retry succeeded" "$mixed_output"; then
  fail "mixed Rekor digest case did not print the verifier success output."
fi

attempt_file="${work_dir}/digest-mismatch-count"
set_attempt_count 0
digest_mismatch_output="${work_dir}/digest-mismatch-output.txt"
set +e
run_release_verification_with_retry \
  "mock Rekor verification" \
  4 \
  1 \
  1 \
  digest_mismatch_with_rekor > "$digest_mismatch_output" 2>&1
digest_mismatch_status=$?
set -e

if [[ "$digest_mismatch_status" -eq 0 ]]; then
  fail "digest mismatch case should fail."
fi
if [[ "$(attempt_count)" != "1" ]]; then
  fail "digest mismatch case should fail fast after one attempt."
fi
if ! grep -q "non-retryable verification error" "$digest_mismatch_output"; then
  fail "digest mismatch case did not report a non-retryable error."
fi

attempt_file="${work_dir}/checkpoint-root-hash-count"
set_attempt_count 0
checkpoint_root_hash_output="${work_dir}/checkpoint-root-hash-output.txt"
run_release_verification_with_retry \
  "mock Rekor verification" \
  3 \
  1 \
  1 \
  checkpoint_root_hash_then_success > "$checkpoint_root_hash_output" 2>&1

if [[ "$(attempt_count)" != "2" ]]; then
  fail "checkpoint root hash case should verify on the second attempt."
fi
if ! grep -q "checkpoint retry succeeded" "$checkpoint_root_hash_output"; then
  fail "checkpoint root hash case did not print the verifier success output."
fi

attempt_file="${work_dir}/backoff-count"
set_attempt_count 0
sleep_log="${work_dir}/sleep-log.txt"
: > "$sleep_log"
backoff_output="${work_dir}/backoff-output.txt"
RELEASE_VERIFICATION_SLEEP_COMMAND=capture_sleep_delay
run_release_verification_with_retry \
  "mock Rekor verification" \
  4 \
  1 \
  2 \
  retryable_until_fourth_attempt > "$backoff_output" 2>&1
RELEASE_VERIFICATION_SLEEP_COMMAND=:

expected_sleep_log="${work_dir}/expected-sleep-log.txt"
printf '1\n2\n2\n' > "$expected_sleep_log"
if [[ "$(attempt_count)" != "4" ]]; then
  fail "backoff case should verify on the fourth attempt."
fi
if ! cmp -s "$expected_sleep_log" "$sleep_log"; then
  echo "expected sleep delays:" >&2
  cat "$expected_sleep_log" >&2
  echo "actual sleep delays:" >&2
  cat "$sleep_log" >&2
  fail "backoff case did not cap retry delays."
fi
if ! grep -q "backoff retry succeeded" "$backoff_output"; then
  fail "backoff case did not print the verifier success output."
fi

attempt_file="${work_dir}/unknown-count"
set_attempt_count 0
unknown_output="${work_dir}/unknown-output.txt"
set +e
run_release_verification_with_retry \
  "mock PyPI verification" \
  4 \
  1 \
  1 \
  unknown_verifier_error > "$unknown_output" 2>&1
unknown_status=$?
set -e

if [[ "$unknown_status" -eq 0 ]]; then
  fail "unknown verifier error case should fail."
fi
if [[ "$(attempt_count)" != "1" ]]; then
  fail "unknown verifier error case should fail fast after one attempt."
fi
if ! grep -q "non-retryable verification error" "$unknown_output"; then
  fail "unknown verifier error case did not report a non-retryable error."
fi

echo "release verification retry tests passed."
