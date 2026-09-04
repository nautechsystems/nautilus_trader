#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo="${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"
run_id="${SECURITY_AUDIT_RUN_ID:-${GITHUB_RUN_ID:?GITHUB_RUN_ID is required}}"
sha="${SECURITY_AUDIT_SHA:-${GITHUB_SHA:?GITHUB_SHA is required}}"
event_name="${SECURITY_AUDIT_EVENT_NAME:-${GITHUB_EVENT_NAME:?GITHUB_EVENT_NAME is required}}"
result="${SECURITY_AUDIT_RESULT:?SECURITY_AUDIT_RESULT is required}"

require_completed_audits() {
  local run_id=$1
  local audit_steps
  local job
  local step
  local conclusion
  local audit_failed=false

  audit_steps="$(
    # shellcheck disable=SC2016 # jq variables must remain literal.
    gh api --method GET "repos/${repo}/actions/runs/${run_id}/jobs" \
      -f "per_page=100" \
      --jq '
        .jobs[]
        | .name as $job
        | .steps[]?
        | [$job, .name, (.conclusion // "")]
        | join("|")
      '
  )"

  # Keep in sync with the two audit jobs and their "Run ..." steps in security-audit.yml.
  while IFS='|' read -r job step; do
    conclusion="$(
      printf '%s\n' "$audit_steps" |
        awk -F '|' -v job="$job" -v step="$step" \
          '$2 == step && ($1 == job || $1 ~ (" / " job "$")) { print $3; exit }'
    )"

    case "$conclusion" in
      success) ;;
      failure) audit_failed=true ;;
      *)
        echo "::error::Security audit step ${job} / ${step} did not complete or its workflow name changed; blocking override" >&2
        return 1
        ;;
    esac
  done << 'AUDIT_STEPS'
zizmor|Run zizmor
supply-chain|Run supply-chain audits
AUDIT_STEPS

  if [[ "$audit_failed" != "true" ]]; then
    echo "::error::No completed audit step reported a failure; blocking override" >&2
    return 1
  fi
}

if [[ "$event_name" != "push" ]]; then
  echo "::error::Security audit publication gate is push-only, was ${event_name}" >&2
  exit 1
fi

case "$result" in
  success)
    echo "Security audit completed successfully"
    exit 0
    ;;
  failure)
    require_completed_audits "$run_id"
    ;;
  *)
    echo "::error::Security audit concluded ${result}; this result cannot be overridden" >&2
    exit 1
    ;;
esac

SECURITY_GATE_RESULT="$result" \
  SECURITY_GATE_SHA="$sha" \
  bash "${script_dir}/check-security-gate-result.bash"
