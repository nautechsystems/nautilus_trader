#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo="${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"
sha="${SECURITY_AUDIT_SHA:-${GITHUB_SHA:?GITHUB_SHA is required}}"
branch="${SECURITY_AUDIT_BRANCH:-${GITHUB_REF_NAME:?GITHUB_REF_NAME is required}}"
workflow="${SECURITY_AUDIT_WORKFLOW:-security-audit.yml}"
event_name="${SECURITY_AUDIT_EVENT_NAME:-${GITHUB_EVENT_NAME:?GITHUB_EVENT_NAME is required}}"
timeout_seconds="${SECURITY_AUDIT_TIMEOUT_SECONDS:-1800}"
poll_seconds="${SECURITY_AUDIT_POLL_SECONDS:-15}"

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

  # Keep in sync with the six audit jobs and their "Run ..." steps in security-audit.yml.
  while IFS='|' read -r job step; do
    conclusion="$(
      printf '%s\n' "$audit_steps" |
        awk -F '|' -v job="$job" -v step="$step" \
          '$1 == job && $2 == step { print $3; exit }'
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
cargo-audit|Run cargo-audit
cargo-deny|Run cargo-deny (advisories, licenses, sources, bans)
cargo-vet|Run cargo-vet
pip-audit|Run pip-audit
osv-scanner|Run osv-scanner
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

if ! [[ "$timeout_seconds" =~ ^[0-9]+$ && "$poll_seconds" =~ ^[1-9][0-9]*$ ]]; then
  echo "::error::Security audit timeout must be non-negative and poll interval must be positive" >&2
  exit 1
fi

deadline=$((SECONDS + timeout_seconds))
last_state="missing"

while true; do
  run_fields="$(
    gh api --method GET "repos/${repo}/actions/workflows/${workflow}/runs" \
      -f "branch=${branch}" \
      -f "event=push" \
      -f "head_sha=${sha}" \
      -f "per_page=20" \
      --jq '
        .workflow_runs
        | sort_by(.created_at)
        | reverse
        | .[0]
        | select(. != null)
        | [.id, .status, (.conclusion // ""), .html_url, .created_at]
        | join("|")
      '
  )"

  if [[ -n "$run_fields" ]]; then
    IFS='|' read -r run_id status conclusion html_url created_at <<< "$run_fields"
    last_state="${status}/${conclusion:-none}"

    echo "Found security-audit run ${run_id} (${last_state}) from ${created_at}"
    echo "$html_url"

    if [[ "$status" == "completed" ]]; then
      if [[ "$conclusion" == "success" ]]; then
        echo "Security audit completed successfully"
        exit 0
      fi

      if [[ "$conclusion" == "failure" ]]; then
        require_completed_audits "$run_id" || exit 1
      fi

      if SECURITY_GATE_RESULT="${conclusion:-unknown}" \
        SECURITY_GATE_SHA="$sha" \
        bash "${script_dir}/check-security-gate-result.bash"; then
        exit 0
      fi

      exit 1
    fi
  else
    echo "Waiting for security-audit push run for ${sha} on ${branch}"
  fi

  if ((SECONDS >= deadline)); then
    echo "::error::Security audit timed out after ${timeout_seconds}s; last state was ${last_state}" >&2
    exit 1
  fi

  echo "Security audit is ${last_state}; checking again in ${poll_seconds}s"
  sleep "$poll_seconds"
done
