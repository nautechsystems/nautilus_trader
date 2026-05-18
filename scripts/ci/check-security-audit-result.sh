#!/usr/bin/env bash
set -euo pipefail

repo="${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"
sha="${SECURITY_AUDIT_SHA:-${GITHUB_SHA:?GITHUB_SHA is required}}"
branch="${SECURITY_AUDIT_BRANCH:-${GITHUB_REF_NAME:?GITHUB_REF_NAME is required}}"
workflow="${SECURITY_AUDIT_WORKFLOW:-security-audit.yml}"

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
      | @tsv
    '
)"

if [[ -z "$run_fields" ]]; then
  echo "No security-audit push run found for ${sha} on ${branch}; allowing publish"
  exit 0
fi

IFS=$'\t' read -r run_id status conclusion html_url created_at <<< "$run_fields"

echo "Found security-audit run ${run_id} (${status}/${conclusion:-none}) from ${created_at}"
echo "$html_url"

if [[ "$status" != "completed" ]]; then
  echo "Security audit is not complete; allowing publish without waiting"
  exit 0
fi

case "$conclusion" in
  success | skipped | neutral)
    echo "Security audit did not fail; allowing publish"
    ;;
  *)
    echo "::error::Security audit concluded ${conclusion:-unknown}; blocking publish"
    exit 1
    ;;
esac
