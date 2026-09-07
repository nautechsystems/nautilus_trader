#!/usr/bin/env bash
set -euo pipefail

pkg_dir=$1
examples_dir=$2
project_dir=${3:-$pkg_dir}

VIRTUAL_ENV="" uv run --project "$project_dir" --no-sync python -m ty check \
  --python-version 3.12 \
  --extra-search-path "$pkg_dir/../docs/tutorials" \
  --extra-search-path "$examples_dir/live/architect_ax" \
  --extra-search-path "$examples_dir/live/interactive_brokers" \
  --extra-search-path "$examples_dir/live/interactive_brokers/notebooks" \
  --extra-search-path "$examples_dir/other/minimal_reproducible_example" \
  "$examples_dir"
