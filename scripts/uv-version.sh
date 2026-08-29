#!/usr/bin/env bash
set -euo pipefail

# Extract the exact uv install version through the shared tool reader
#
# Usage: uv-version.sh
# Example output: 0.12.6

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

exec bash "${SCRIPT_DIR}/tool-version.sh" uv
