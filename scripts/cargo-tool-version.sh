#!/usr/bin/env bash
set -euo pipefail

# Extract a Cargo tool version from the shared catalog or local Cargo metadata
#
# Usage: cargo-tool-version.sh <tool-name>
# Example: cargo-tool-version.sh cargo-vet
#          cargo-tool-version.sh lychee

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
CARGO_TOML="${REPO_ROOT}/Cargo.toml"

if [[ -n "${NAUTILUS_ENGINEERING_TOOLS_FILE:-}" ]]; then
  SHARED_TOOLS_TOML=$NAUTILUS_ENGINEERING_TOOLS_FILE
elif [[ -f "${REPO_ROOT}/.nautilus-engineering/tools.toml" ]]; then
  SHARED_TOOLS_TOML="${REPO_ROOT}/.nautilus-engineering/tools.toml"
else
  SHARED_TOOLS_TOML="${REPO_ROOT}/tools.toml"
fi

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 <tool-name>" >&2
  echo "Example: $0 cargo-vet" >&2
  exit 1
fi

TOOL_NAME="$1"

if [[ ! "$TOOL_NAME" =~ ^[a-z0-9][a-z0-9_-]*$ ]]; then
  echo "Error: Invalid cargo tool name: $TOOL_NAME" >&2
  exit 1
fi

if [[ ! -f "$SHARED_TOOLS_TOML" ]]; then
  echo "Error: shared tools catalog not found at $SHARED_TOOLS_TOML" >&2
  exit 1
fi

SHARED_VERSION=$(awk -v section="[$TOOL_NAME]" '
  $0 == section { in_section=1; next }
  /^\[/ { in_section=0 }
  in_section && /^version[[:space:]]*=/ {
    gsub(/.*=[[:space:]]*"/, "")
    gsub(/".*/, "")
    print
    exit
  }
' "$SHARED_TOOLS_TOML")

LOCAL_VERSION=""
if [[ -f "$CARGO_TOML" ]]; then
  LOCAL_VERSION=$(awk -v tool="$TOOL_NAME" '
  /^\[workspace\.metadata\.tools\]/ { in_section=1; next }
  /^\[/ { in_section=0 }
  in_section && $1 == tool { gsub(/[" ]/, "", $3); print $3; exit }
  ' "$CARGO_TOML")
fi

if [[ -n "$SHARED_VERSION" && -n "$LOCAL_VERSION" ]]; then
  echo "Error: Duplicate Cargo tool version for $TOOL_NAME in shared catalog and Cargo.toml" >&2
  exit 1
fi

VERSION=${SHARED_VERSION:-$LOCAL_VERSION}

if [[ -z "$VERSION" ]]; then
  echo "Error: Could not find $TOOL_NAME in the shared catalog or [workspace.metadata.tools]" >&2
  exit 1
fi

if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?(\+[0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?$ ]]; then
  echo "Error: Invalid version for $TOOL_NAME: $VERSION" >&2
  exit 1
fi

echo -n "$VERSION"
