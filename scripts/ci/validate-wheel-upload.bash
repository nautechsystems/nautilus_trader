#!/usr/bin/env bash
set -euo pipefail

dist_dir=${1:-dist}
pyproject=${2:-python/pyproject.toml}

if [ ! -d "$dist_dir" ]; then
  echo "Error: dist directory not found: $dist_dir" >&2
  exit 1
fi

shopt -s nullglob
asset_paths=("$dist_dir"/nautilus_trader-*.whl)
shopt -u nullglob

if ((${#asset_paths[@]} != 1)); then
  echo "Error: Expected one nautilus_trader wheel in $dist_dir, found ${#asset_paths[@]}" >&2
  exit 1
fi

asset_path=${asset_paths[0]}
if [ ! -f "$asset_path" ]; then
  echo "Error: Wheel artifact is not a file: $asset_path" >&2
  exit 1
fi

asset_name=$(basename "$asset_path")
wheel_pattern='^nautilus_trader-([0-9A-Za-z][0-9A-Za-z._+]*)-(cp[0-9]+)-([A-Za-z0-9_.]+)-([A-Za-z0-9_.]+)\.whl$'
if [[ ! "$asset_name" =~ $wheel_pattern ]]; then
  echo "Error: Invalid wheel filename: $asset_name" >&2
  exit 1
fi

expected_version=$(awk -F '"' '/^version = / { print $2; exit }' "$pyproject")
wheel_version=${BASH_REMATCH[1]}
python_tag=${BASH_REMATCH[2]}
abi_tag=${BASH_REMATCH[3]}
platform_tag=${BASH_REMATCH[4]}

if [ -z "$expected_version" ] || [ "$wheel_version" != "$expected_version" ]; then
  echo "Error: Wheel version $wheel_version does not match package version $expected_version" >&2
  exit 1
fi
if [[ ! "$python_tag" =~ ^cp(312|313|314)$ ]] || [ "$abi_tag" != "$python_tag" ]; then
  echo "Error: Wheel has unsupported Python or ABI tags: $python_tag-$abi_tag" >&2
  exit 1
fi

platform_pattern='^(manylinux_[0-9]+_[0-9]+_(x86_64|aarch64)|macosx_[0-9]+_[0-9]+_arm64|win_amd64)$'
if [[ ! "$platform_tag" =~ $platform_pattern ]]; then
  echo "Error: Wheel has unsupported platform tag: $platform_tag" >&2
  exit 1
fi

printf '%s\n' "$asset_name"
