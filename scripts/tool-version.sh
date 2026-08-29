#!/usr/bin/env bash
set -euo pipefail

# Extract a tool version from the shared catalog or a consumer-local tools.toml
#
# Usage: tool-version.sh <tool-name>
# Example: tool-version.sh prek  ->  0.4.14

if (($# != 1)); then
  echo "Usage: tool-version.sh <tool-name>" >&2
  exit 2
fi

TOOL_NAME=$1

if [[ ! "$TOOL_NAME" =~ ^[a-z0-9][a-z0-9_-]*$ ]]; then
  echo "Error: Invalid tool name: $TOOL_NAME" >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

if [[ -n "${NAUTILUS_ENGINEERING_TOOLS_FILE:-}" ]]; then
  SHARED_TOOLS_TOML=$NAUTILUS_ENGINEERING_TOOLS_FILE
elif [[ -f "${REPO_ROOT}/.nautilus-engineering/tools.toml" ]]; then
  SHARED_TOOLS_TOML="${REPO_ROOT}/.nautilus-engineering/tools.toml"
else
  SHARED_TOOLS_TOML="${REPO_ROOT}/tools.toml"
fi
LOCAL_TOOLS_TOML="${REPO_ROOT}/tools.toml"

if [[ ! -f "$SHARED_TOOLS_TOML" ]]; then
  echo "Error: shared tools catalog not found at $SHARED_TOOLS_TOML" >&2
  exit 1
fi

read_version() {
  local catalog=$1

  awk -v section="[$TOOL_NAME]" '
    $0 == section { in_section=1; next }
    /^\[/ { in_section=0 }
    in_section && /^version[[:space:]]*=/ {
      gsub(/.*=[[:space:]]*"/, "")
      gsub(/".*/, "")
      print
      exit
    }
  ' "$catalog"
}

SHARED_VERSION=$(read_version "$SHARED_TOOLS_TOML")
LOCAL_VERSION=""
if [[ -f "$LOCAL_TOOLS_TOML" && ! "$LOCAL_TOOLS_TOML" -ef "$SHARED_TOOLS_TOML" ]]; then
  LOCAL_VERSION=$(read_version "$LOCAL_TOOLS_TOML")
fi

if [[ -n "$SHARED_VERSION" && -n "$LOCAL_VERSION" ]]; then
  echo "Error: Duplicate tool version for [$TOOL_NAME] in shared and local catalogs" >&2
  exit 1
fi

VERSION=${SHARED_VERSION:-$LOCAL_VERSION}

if [[ -z "$VERSION" ]]; then
  echo "Error: Could not find version for [$TOOL_NAME] in shared or local tools.toml" >&2
  exit 1
fi

if [[ ! "$VERSION" =~ ^([0-9]+\.[0-9]+\.[0-9]+(\.[0-9]+)*(-[0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?(\+[0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?|nightly-[0-9]{4}-[0-9]{2}-[0-9]{2})$ ]]; then
  echo "Error: Invalid version for [$TOOL_NAME]: $VERSION" >&2
  exit 1
fi

echo -n "$VERSION"
