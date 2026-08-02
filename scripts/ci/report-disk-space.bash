#!/usr/bin/env bash

set -euo pipefail

label=${1:?Usage: report-disk-space.bash LABEL}
target_dir=${CARGO_TARGET_DIR:-target}
disk_path=/

echo "::group::Disk space ${label}"
df -h /
if [ -d "$target_dir" ]; then
  du -sh "$target_dir" || true
  disk_path=$target_dir
  df -h "$disk_path"
fi
echo "::endgroup::"

available_gb=$(($(df -Pk "$disk_path" | awk 'NR == 2 {print $4}') / 1024 / 1024))
echo "Available on the filesystem containing ${disk_path} ${label}: ${available_gb}G"
