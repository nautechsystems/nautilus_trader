#!/usr/bin/env bash
set -euo pipefail

# shellcheck source=scripts/native-path.bash
source "$(dirname "${BASH_SOURCE[0]}")/../native-path.bash"

pkg_dir="$(native_path "$1")"
examples_dir="$(native_path "$2")"
project_dir="$(native_path "${3:-$1}")"

VIRTUAL_ENV="" uv run --project "$project_dir" --no-sync python -m ty check \
  --python-version 3.12 \
  --extra-search-path "$pkg_dir/../docs/tutorials" \
  --extra-search-path "$examples_dir/live/architect_ax" \
  --extra-search-path "$examples_dir/live/interactive_brokers" \
  --extra-search-path "$examples_dir/live/interactive_brokers/notebooks" \
  --extra-search-path "$examples_dir/other/minimal_reproducible_example" \
  "$examples_dir"
