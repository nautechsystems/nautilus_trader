#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 <version>" >&2
  exit 1
fi

version=$1
if [[ "$version" =~ ^[0-9]+(\.[0-9]+){2}$ ]]; then
  echo "release"
elif [[ "$version" =~ ^[0-9]+(\.[0-9]+){2}(a|b|rc)[0-9]+$ ]]; then
  echo "prerelease"
else
  echo "Error: Release version must be a release or pre-release, was ${version}" >&2
  exit 1
fi
