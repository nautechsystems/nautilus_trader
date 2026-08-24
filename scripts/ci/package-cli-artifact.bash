#!/usr/bin/env bash
set -euo pipefail

target=${1:?Usage: package-cli-artifact.bash TARGET}
stage_dir=stage-cli

if [[ ! "$target" =~ ^[0-9A-Za-z_.-]+$ ]]; then
  echo "Invalid CLI target: $target" >&2
  exit 1
fi

mkdir -p "$stage_dir" dist
cp target/release/nautilus "$stage_dir/nautilus"
cp -L crates/cli/LICENSE "$stage_dir/LICENSE" || cp -L LICENSE "$stage_dir/LICENSE"
cp crates/cli/README.md "$stage_dir/README.md"
tar -C "$stage_dir" -czf "dist/nautilus-${target}.tar.gz" .
rm -rf "$stage_dir"
