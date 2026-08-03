#!/usr/bin/env bash
set -euo pipefail

repo="${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"
sha="${SECURITY_AUDIT_SHA:-${GITHUB_SHA:?GITHUB_SHA is required}}"
branch="${SECURITY_AUDIT_BRANCH:-${GITHUB_REF_NAME:?GITHUB_REF_NAME is required}}"
workflow="${SECURITY_AUDIT_WORKFLOW:-security-audit.yml}"
event_name="${SECURITY_AUDIT_EVENT_NAME:-${GITHUB_EVENT_NAME:?GITHUB_EVENT_NAME is required}}"
timeout_seconds="${SECURITY_AUDIT_TIMEOUT_SECONDS:-1800}"
poll_seconds="${SECURITY_AUDIT_POLL_SECONDS:-15}"

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

      echo "::error::Security audit concluded ${conclusion:-unknown}; blocking publish" >&2
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
