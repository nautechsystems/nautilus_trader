#!/usr/bin/env bash
set -euo pipefail

pkg_dir=$1
examples_dir=$2

VIRTUAL_ENV="" uv run --project "$pkg_dir" --no-sync ty check \
  --python-version 3.12 \
  "$examples_dir/__init__.py" \
  "$examples_dir"/live/*/__init__.py \
  "$examples_dir"/live/*/data_tester.py \
  "$examples_dir"/live/*/exec_tester.py \
  "$examples_dir/live/blockchain/actors.py" \
  "$examples_dir/live/blockchain/node_test.py" \
  "$examples_dir/live/lighter/nvda_composite_mm.py" \
  "$examples_dir/live/polymarket/updown_smoke_tester.py"
