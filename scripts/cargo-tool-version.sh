#!/bin/bash
set -euo pipefail

# Extract a cargo tool version from Cargo.toml [workspace.metadata.tools]
#
# Usage: cargo-tool-version.sh <tool-name>
# Example: cargo-tool-version.sh cargo-vet
#          cargo-tool-version.sh cargo-deny

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CARGO_TOML="${SCRIPT_DIR}/../Cargo.toml"

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 <tool-name>" >&2
  echo "Example: $0 cargo-vet" >&2
  exit 1
fi

TOOL_NAME="$1"

if [[ ! -f "$CARGO_TOML" ]]; then
  echo "Error: Cargo.toml not found at $CARGO_TOML" >&2
  exit 1
fi

# Extract version from [workspace.metadata.tools] section
# Matches: tool-name = "version"
VERSION=$(awk -v tool="$TOOL_NAME" '
  /^\[workspace\.metadata\.tools\]/ { in_section=1; next }
  /^\[/ { in_section=0 }
  in_section && $1 == tool { gsub(/[" ]/, "", $3); print $3; exit }
' "$CARGO_TOML")

if [[ -z "$VERSION" ]]; then
  echo "Error: Could not find $TOOL_NAME in [workspace.metadata.tools]" >&2
  exit 1
fi

echo -n "$VERSION"
