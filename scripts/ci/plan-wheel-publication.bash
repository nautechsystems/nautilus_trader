#!/usr/bin/env bash
set -euo pipefail

output_file="${GITHUB_OUTPUT:?GITHUB_OUTPUT is required}"
event_name="${EVENT_NAME:?EVENT_NAME is required}"
ref_name="${REF_NAME:?REF_NAME is required}"
publish_date="${PUBLISH_DATE:-$(date -u +%Y%m%d)}"
mode="$(bash scripts/ci/publish-wheels-policy.bash "$event_name" "$ref_name")"

if ! [[ "$publish_date" =~ ^[0-9]{8}$ ]]; then
  echo "Error: PUBLISH_DATE must use YYYYMMDD format, was ${publish_date}" >&2
  exit 1
fi

publish_r2=false
publish_development=false
publish_environment=""
wheel_version=""

if [[ "$mode" != "none" ]]; then
  current_version="$(awk -F '"' '/^version = / { print $2; exit }' python/pyproject.toml)"
  if [[ -z "$current_version" ]]; then
    echo "Error: Failed to extract the Python package version" >&2
    exit 1
  fi

  base_version="$(printf '%s\n' "$current_version" |
    sed -E 's/(\.dev[0-9]{8}(\+[0-9]+)?|a[0-9]{8})$//')"
  publish_r2=true

  if [[ "$mode" == "development" ]]; then
    run_number="${GITHUB_RUN_NUMBER:?GITHUB_RUN_NUMBER is required for development wheels}"
    if ! [[ "$run_number" =~ ^[0-9]+$ ]]; then
      echo "Error: GITHUB_RUN_NUMBER must be numeric, was ${run_number}" >&2
      exit 1
    fi

    publish_development=true
    publish_environment="r2-develop"
    wheel_version="${base_version}.dev${publish_date}+${run_number}"
  else
    publish_environment="r2-nightly"
    if [[ "$base_version" =~ (a|b|rc)[0-9]+$ ]]; then
      wheel_version="${base_version}.dev${publish_date}"
    else
      wheel_version="${base_version}a${publish_date}"
    fi
  fi
fi

{
  echo "publish_r2=${publish_r2}"
  echo "publish_development=${publish_development}"
  echo "publish_environment=${publish_environment}"
  echo "wheel_matrix=${mode}"
  echo "wheel_version=${wheel_version}"
} >> "$output_file"

echo "Wheel publication mode: ${mode}"
if [[ -n "$wheel_version" ]]; then
  echo "Wheel publication version: ${wheel_version}"
fi
