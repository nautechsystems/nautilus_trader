#!/usr/bin/env bash
set -euo pipefail

current_version=$(grep '^version = ' pyproject.toml | cut -d '"' -f2)
if [[ -z "$current_version" ]]; then
  echo "Error: Failed to extract version from pyproject.toml" >&2
  exit 1
fi

branch_name="${GITHUB_REF_NAME}" # Get the branch name
echo "Branch name: ${branch_name}"
base_version=$(echo "$current_version" | sed -E 's/(\.dev[0-9]{8}\+[0-9]+|a[0-9]{8})$//')

suffix=""
if [[ "$branch_name" == "develop" ]]; then
  # Develop branch: use dev versioning with build number
  suffix=".dev$(date +%Y%m%d)+${GITHUB_RUN_NUMBER}"
elif [[ "$branch_name" == "nightly" ]]; then
  # Nightly branch: use alpha versioning
  suffix="a$(date +%Y%m%d)"
else
  echo "Not modifying version"
fi

if [[ -n "$suffix" && "$current_version" != *"$suffix"* ]]; then
  new_version="${base_version}${suffix}"
  if sed -i.bak "s/^version = \".*\"/version = \"${new_version}\"/" pyproject.toml; then
    echo "Version updated to ${new_version}"
    rm -f pyproject.toml.bak
  else
    echo "Error: Failed to update version in pyproject.toml" >&2
    exit 1
  fi
fi
