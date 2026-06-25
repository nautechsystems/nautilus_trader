#!/usr/bin/env bash
set -euo pipefail

current_version=$(grep '^version = ' pyproject.toml | cut -d '"' -f2)
if [[ -z "$current_version" ]]; then
  echo "Error: Failed to extract version from pyproject.toml" >&2
  exit 1
fi

branch_name="${GITHUB_REF_NAME}" # Get the branch name
echo "Branch name: ${branch_name}"
base_version=$(echo "$current_version" | sed -E 's/(\.dev[0-9]{8}(\+[0-9]+)?|a[0-9]{8})$//')

suffix=""
if [[ "$branch_name" == "develop" ]]; then
  # Develop branch: use dev versioning with build number
  suffix=".dev$(date +%Y%m%d)+${GITHUB_RUN_NUMBER}"
elif [[ "$branch_name" == "nightly" ]]; then
  # Nightly branch: use alpha versioning, unless the base is already a pre-release
  # (e.g. 2.0.0rc1). PEP 440 forbids stacking pre-release segments (2.0.0rc1a... is
  # invalid), so fall back to a dev segment when the base already has one.
  if [[ "$base_version" =~ (a|b|rc)[0-9]+$ ]]; then
    suffix=".dev$(date +%Y%m%d)"
  else
    suffix="a$(date +%Y%m%d)"
  fi
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
