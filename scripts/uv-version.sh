#!/usr/bin/env bash
set -euo pipefail

# Extract the exact uv install version from tools.toml.
#
# Usage: uv-version.sh
# Example output: 0.12.3

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

exec bash "${SCRIPT_DIR}/tool-version.sh" uv
