#!/usr/bin/env bash
set -euo pipefail

workflow_runs_path=${1:?Usage: check-nightly-merge-status.bash WORKFLOW_RUNS_PATH DEVELOP_SHA}
develop_sha=${2:?Usage: check-nightly-merge-status.bash WORKFLOW_RUNS_PATH DEVELOP_SHA}

if [[ ! "$develop_sha" =~ ^[0-9a-f]{40}$ ]]; then
  echo "ERROR: Invalid develop commit: $develop_sha" >&2
  exit 2
fi

if ! jq -e --arg develop_sha "$develop_sha" '
  def nonnegative_integer:
    type == "number" and . >= 0 and (. | floor) == .;
  def positive_integer:
    type == "number" and . > 0 and (. | floor) == .;

  (.total_count | nonnegative_integer)
  and (.workflow_runs | type == "array")
  and (.total_count == (.workflow_runs | length))
  and all(.workflow_runs[];
    (.name | type == "string")
    and (.head_branch | type == "string")
    and .head_sha == $develop_sha
    and (.event | type == "string")
    and (.run_number | positive_integer)
    and (.status | type == "string")
    and has("conclusion")
    and (.conclusion == null or (.conclusion | type == "string"))
  )
' "$workflow_runs_path" > /dev/null; then
  echo "ERROR: Invalid or incomplete workflow runs for develop commit $develop_sha" >&2
  exit 2
fi

if jq -e '
  any(.workflow_runs[];
    .name == "build"
    and .head_branch == "develop"
    and .event == "push"
    and .status == "completed"
    and .conclusion == "success"
  )
' "$workflow_runs_path" > /dev/null; then
  exit 0
fi

exit 1
