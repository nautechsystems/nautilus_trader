#!/usr/bin/env bash
set -euo pipefail

pkg_dir=$1
examples_dir=$2

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
UV_PROJECT_ENVIRONMENT="$(bash "$script_dir/../uv-project-environment.bash")"
export UV_PROJECT_ENVIRONMENT

VIRTUAL_ENV="" uv run --project "$pkg_dir" --no-sync ty check \
  --python-version 3.12 \
  --extra-search-path "$pkg_dir/../docs/tutorials" \
  --extra-search-path "$examples_dir/live/architect_ax" \
  --extra-search-path "$examples_dir/live/interactive_brokers" \
  --extra-search-path "$examples_dir/live/interactive_brokers/notebooks" \
  --extra-search-path "$examples_dir/other/minimal_reproducible_example" \
  "$examples_dir"
