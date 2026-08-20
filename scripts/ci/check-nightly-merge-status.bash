#!/usr/bin/env bash
set -euo pipefail

workflow_runs_path="${1:-workflow_runs.json}"

if [[ -z "${GITHUB_OUTPUT:-}" ]]; then
  echo "::error::GITHUB_OUTPUT is required" >&2
  exit 1
fi

if ! successful_workflow=$(jq -ce '
  .workflow_runs
  | select(type == "array")
  | map(
      select(
        .name == "build"
        and .head_branch == "develop"
        and .event == "push"
        and .status == "completed"
        and .conclusion == "success"
      )
    )
  | select(
      all(.[];
        (try (
          (.created_at | type) == "string"
          and (.created_at | fromdateiso8601 | type) == "number"
        ) catch false)
        and (try (
          (.run_number | type) == "number"
          and .run_number > 0
          and (.run_number | floor) == .run_number
        ) catch false)
      )
    )
  | sort_by([.created_at, .run_number])
  | last // empty
' "$workflow_runs_path"); then
  echo "::error::Expected a successful develop build workflow" >&2
  exit 1
fi

develop_sha=$(jq -r '.head_sha' <<< "$successful_workflow")
if [[ ! "$develop_sha" =~ ^[0-9a-f]{40}$ ]]; then
  echo "::error::Successful develop build has an invalid head SHA" >&2
  exit 1
fi

echo "Last successful workflow:"
echo "$successful_workflow" | jq '{id, run_number, head_sha, created_at, updated_at}'
echo "Last successful develop commit: $develop_sha"

nightly_sha=$(git rev-parse --verify 'refs/remotes/origin/nightly^{commit}')
current_develop_sha=$(git rev-parse --verify 'refs/remotes/origin/develop^{commit}')
echo "Current nightly HEAD: $nightly_sha"
echo "Current develop HEAD: $current_develop_sha"

write_outputs() {
  printf 'sha=%s\nhas_changes=%s\n' "$develop_sha" "$1" >> "$GITHUB_OUTPUT"
}

is_ancestor() {
  local ancestor="$1"
  local descendant="$2"
  local status

  if git merge-base --is-ancestor "$ancestor" "$descendant"; then
    return 0
  else
    status=$?
  fi

  if [[ "$status" -eq 1 ]]; then
    return 1
  fi

  echo "::error::Failed to compare commits $ancestor and $descendant" >&2
  exit "$status"
}

if ! is_ancestor "$nightly_sha" "$current_develop_sha" &&
  ! is_ancestor "$current_develop_sha" "$nightly_sha"; then
  echo "::error::Nightly has diverged from current origin/develop" >&2
  exit 1
fi

if [[ "$nightly_sha" != "$current_develop_sha" ]] &&
  is_ancestor "$current_develop_sha" "$nightly_sha"; then
  echo "::warning::Nightly is ahead of current origin/develop; leaving nightly unchanged"
  write_outputs false
  exit 0
fi

if ! git cat-file -e "${develop_sha}^{commit}" 2> /dev/null; then
  echo "::warning::GitHub Actions returned a build commit absent from the fetched develop history; leaving nightly unchanged"
  write_outputs false
  exit 0
fi

if [[ "$nightly_sha" == "$develop_sha" ]]; then
  echo "Nightly is already at the last successful develop commit"
  write_outputs false
elif is_ancestor "$nightly_sha" "$develop_sha"; then
  if is_ancestor "$develop_sha" "$current_develop_sha"; then
    echo "Develop has new successful changes to merge"
    write_outputs true
  else
    echo "::warning::GitHub Actions returned a build commit outside the current develop history; leaving nightly unchanged"
    write_outputs false
  fi
elif is_ancestor "$develop_sha" "$nightly_sha"; then
  echo "::warning::GitHub Actions returned stale or out-of-order build status; leaving nightly unchanged"
  write_outputs false
else
  echo "::warning::GitHub Actions returned a build commit outside nightly's fast-forward path; leaving nightly unchanged"
  write_outputs false
fi
