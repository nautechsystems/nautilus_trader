#!/usr/bin/env bash
set -euo pipefail

develop_sha=${1:?Usage: merge-nightly.bash DEVELOP_SHA}
if [[ ! "$develop_sha" =~ ^[0-9a-f]{40}$ ]]; then
  echo "ERROR: Invalid develop commit: $develop_sha" >&2
  exit 1
fi

echo "Merging develop commit $develop_sha into nightly"
if ! git ls-remote --exit-code --heads origin nightly; then
  echo "ERROR: nightly branch does not exist" >&2
  exit 1
fi

git fetch origin nightly:nightly
git fetch origin develop
git checkout nightly

if ! git merge --ff-only "$develop_sha"; then
  echo "ERROR: Fast-forward merge failed - nightly may have diverged from develop" >&2
  exit 1
fi

echo "Successfully merged $develop_sha into nightly"
git push origin nightly
echo "Changes pushed to nightly"

nightly_sha=$(git rev-parse HEAD)
if [[ "$nightly_sha" != "$develop_sha" ]]; then
  echo "ERROR: Nightly HEAD does not match expected develop commit" >&2
  exit 1
fi
echo "Nightly is now at develop commit $develop_sha"
