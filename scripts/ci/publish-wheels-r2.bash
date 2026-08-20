#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT
skip_file="${work_dir}/skip"

PUBLISH_WHEELS_SKIP_FILE="$skip_file" bash "${script_dir}/publish-wheels-r2-upload-new-wheels.sh"
if [[ -e "$skip_file" ]]; then
  exit 0
fi
bash "${script_dir}/publish-wheels-r2-remove-old-wheels.sh" plan
bash "${script_dir}/publish-wheels-generate-index.sh"
bash "${script_dir}/publish-wheels-r2-upload-index.sh"
bash "${script_dir}/publish-wheels-r2-verify-files.sh"
bash "${script_dir}/publish-wheels-r2-remove-old-wheels.sh" apply

echo "Wheel publication transaction completed"
