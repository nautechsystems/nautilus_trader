#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

bash "${script_dir}/publish-wheels-r2-upload-new-wheels.sh"
bash "${script_dir}/publish-wheels-r2-remove-old-wheels.sh" plan
bash "${script_dir}/publish-wheels-generate-index.sh"
bash "${script_dir}/publish-wheels-r2-upload-index.sh"
bash "${script_dir}/publish-wheels-r2-verify-files.sh"
bash "${script_dir}/publish-wheels-r2-remove-old-wheels.sh" apply

echo "Wheel publication transaction completed"
