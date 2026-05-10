#!/usr/bin/env bash
set -euo pipefail

# Extract a tool version from tools.toml
#
# Usage: tool-version.sh <tool-name>
# Example: tool-version.sh prek  ->  0.3.8

TOOL_NAME="${1:?Usage: tool-version.sh <tool-name>}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TOOLS_TOML="${SCRIPT_DIR}/../tools.toml"

if [[ ! -f "$TOOLS_TOML" ]]; then
  echo "Error: tools.toml not found at $TOOLS_TOML" >&2
  exit 1
fi

# Parse the version field from the [tool-name] section.
# Handles: version = "1.2.3"
VERSION=$(awk -v section="[$TOOL_NAME]" '
  $0 == section { in_section=1; next }
  /^\[/ { in_section=0 }
  in_section && /^version[[:space:]]*=/ {
    gsub(/.*=[[:space:]]*"/, "")
    gsub(/".*/, "")
    print
    exit
  }
' "$TOOLS_TOML")

if [[ -z "$VERSION" ]]; then
  echo "Error: Could not find version for [$TOOL_NAME] in tools.toml" >&2
  exit 1
fi

echo -n "$VERSION"
