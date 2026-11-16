#!/usr/bin/env bash
set -euo pipefail

: "${GITHUB_OUTPUT:?GITHUB_OUTPUT is required}"

ignore_override() {
  if [[ -n "${1:-}" ]]; then
    echo "::warning::$1; running security audits"
  fi

  echo "override_active=false" >> "$GITHUB_OUTPUT"
  exit 0
}

if [[ "${GITHUB_EVENT_NAME:?GITHUB_EVENT_NAME is required}" != "push" ]] ||
  [[ "${GITHUB_REF:?GITHUB_REF is required}" != "refs/heads/develop" &&
    "$GITHUB_REF" != "refs/heads/nightly" ]]; then
  ignore_override
fi

override="${SECURITY_GATE_OVERRIDE:-}"
if [[ -z "$override" || "$override" == "disabled" ]]; then
  ignore_override
fi

override_pattern='^([0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z)@([0-9a-f]{40})$'
if [[ ! "$override" =~ $override_pattern ]]; then
  ignore_override "Security gate override must use <UTC expiry>@<full commit SHA>"
fi

expiry="${BASH_REMATCH[1]}"
override_sha="${BASH_REMATCH[2]}"
expected_sha="${GITHUB_SHA:?GITHUB_SHA is required}"

if [[ "$override_sha" != "$expected_sha" ]]; then
  ignore_override "Security gate override targets ${override_sha}, expected ${expected_sha}"
fi

expiry_epoch="$(
  date -u -d "$expiry" +%s 2> /dev/null ||
    date -j -u -f '%Y-%m-%dT%H:%M:%SZ' "$expiry" +%s 2> /dev/null ||
    true
)"
if [[ ! "$expiry_epoch" =~ ^[0-9]+$ ]]; then
  ignore_override "Security gate override expiry is not a valid UTC timestamp"
fi

now_epoch="$(date -u +%s)"
remaining_seconds=$((expiry_epoch - now_epoch))
if ((remaining_seconds <= 0)); then
  ignore_override "Security gate override expired at ${expiry}"
fi

max_seconds=7200
if ((remaining_seconds > max_seconds)); then
  ignore_override "Security gate override expiry exceeds the two-hour limit"
fi

echo "::warning::Security gate override accepted for ${expected_sha} until ${expiry}"
echo "override_active=true" >> "$GITHUB_OUTPUT"
