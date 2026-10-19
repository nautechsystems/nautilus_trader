#!/usr/bin/env bash
set -euo pipefail

result="${SECURITY_GATE_RESULT:?SECURITY_GATE_RESULT is required}"

if [[ "$result" == "success" ]]; then
  exit 0
fi

if [[ "$result" != "failure" ]]; then
  echo "::error::Security gate concluded ${result}; this result cannot be overridden" >&2
  exit 1
fi

override="${SECURITY_GATE_OVERRIDE:-}"
if [[ -z "$override" || "$override" == "disabled" ]]; then
  echo "::error::Security gate concluded failure; blocking publish" >&2
  exit 1
fi

override_pattern='^([0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z)@([0-9a-f]{40})$'
if [[ ! "$override" =~ $override_pattern ]]; then
  echo "::error::Security gate override must use <UTC expiry>@<full commit SHA>" >&2
  exit 1
fi

expiry="${BASH_REMATCH[1]}"
override_sha="${BASH_REMATCH[2]}"
expected_sha="${SECURITY_GATE_SHA:-${GITHUB_SHA:?GITHUB_SHA is required}}"

if [[ "$override_sha" != "$expected_sha" ]]; then
  echo "::error::Security gate override targets ${override_sha}, expected ${expected_sha}" >&2
  exit 1
fi

expiry_epoch="$(
  date -u -d "$expiry" +%s 2> /dev/null ||
    date -j -u -f '%Y-%m-%dT%H:%M:%SZ' "$expiry" +%s 2> /dev/null ||
    true
)"
if [[ ! "$expiry_epoch" =~ ^[0-9]+$ ]]; then
  echo "::error::Security gate override expiry is not a valid UTC timestamp" >&2
  exit 1
fi

now_epoch="$(date -u +%s)"
remaining_seconds=$((expiry_epoch - now_epoch))
if ((remaining_seconds <= 0)); then
  echo "::error::Security gate override expired at ${expiry}" >&2
  exit 1
fi

max_seconds=7200
if ((remaining_seconds > max_seconds)); then
  echo "::error::Security gate override expiry exceeds the two-hour limit" >&2
  exit 1
fi

echo "::warning::Security gate override accepted for ${expected_sha} until ${expiry}"
