#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
token=${NIGHTLY_TOKEN:?NIGHTLY_TOKEN is required}
workflow_runs=$(mktemp)
trap 'rm -f "$workflow_runs"' EXIT

url="https://api.github.com/repos/nautechsystems/nautilus_trader/actions/workflows/build.yml/runs"
url="${url}?branch=develop&event=push&per_page=100"
echo "Fetching workflows from: $url"
if ! curl -sS -L \
  --retry 5 --retry-delay 2 --retry-all-errors \
  --connect-timeout 5 --max-time 60 --fail-with-body \
  -H "Accept: application/vnd.github+json" \
  -H "Authorization: Bearer ${token}" \
  -H "X-GitHub-Api-Version: 2022-11-28" \
  "$url" > "$workflow_runs"; then
  echo "Failed to fetch workflows, exiting" >&2
  exit 1
fi

echo "Fetched workflow run summary:"
jq '{total_count, returned: (.workflow_runs | length)}' "$workflow_runs"

if ! git ls-remote --exit-code --heads origin nightly; then
  echo "ERROR: nightly branch does not exist" >&2
  exit 1
fi

git fetch origin \
  +refs/heads/nightly:refs/remotes/origin/nightly \
  +refs/heads/develop:refs/remotes/origin/develop
bash "$script_dir/check-nightly-merge-status.bash" "$workflow_runs"
