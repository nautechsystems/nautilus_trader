#!/usr/bin/env bash

# Regenerate Cap'n Proto schema files
#
# This script regenerates Rust bindings from Cap'n Proto schema files.
# Run this whenever you modify any .capnp schema files.
#
# Requirements:
#   - Cap'n Proto compiler (capnp) must be installed
#   - capnpc-rust (installed via cargo build-dependencies)
#
# Usage:
#   ./scripts/regen_capnp.sh

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Get script directory and project root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# Read required version from capnp-version file
CAPNP_VERSION_FILE="${PROJECT_ROOT}/capnp-version"
if [[ -f "$CAPNP_VERSION_FILE" ]]; then
  REQUIRED_VERSION=$(cat "$CAPNP_VERSION_FILE" | tr -d '[:space:]')
else
  echo -e "${RED}Error: capnp-version file not found at $CAPNP_VERSION_FILE${NC}"
  exit 1
fi

echo -e "${YELLOW}Regenerating Cap'n Proto schemas...${NC}"

# Check if capnp is installed
if ! command -v capnp &> /dev/null; then
  echo -e "${RED}Error: capnp compiler not found${NC}"
  echo "Please install Cap'n Proto ${REQUIRED_VERSION}:"
  echo "  - macOS: brew install capnp"
  echo "  - Linux: Install from source (Ubuntu's package is too old):"
  echo "      ./scripts/install-capnp.sh"
  echo "    Or manually: https://capnproto.org/install.html"
  exit 1
fi

# Verify installed version matches required version
INSTALLED_VERSION=$(capnp --version | awk '{print $NF}')
if [[ "$INSTALLED_VERSION" != "$REQUIRED_VERSION" ]]; then
  echo -e "${RED}Error: capnp version mismatch${NC}"
  echo "  Required: ${REQUIRED_VERSION} (from capnp-version)"
  echo "  Installed: ${INSTALLED_VERSION}"
  echo "Please install the correct version using: ./scripts/install-capnp.sh"
  exit 1
fi

echo "Using capnp: $(command -v capnp)"
echo "Version: $(capnp --version)"

# Navigate to project root
cd "${PROJECT_ROOT}"

# Clean existing generated files
echo -e "${YELLOW}Cleaning existing generated files...${NC}"
rm -rf crates/serialization/generated/capnp/*

# Force a clean rebuild of the serialization crate with capnp feature
echo -e "${YELLOW}Rebuilding serialization crate to regenerate schemas...${NC}"
cargo clean -p nautilus-serialization
cargo build -p nautilus-serialization --features capnp --message-format=json 2>&1 |
  grep -o '"out_dir":"[^"]*"' |
  cut -d'"' -f4 |
  grep nautilus-serialization > /tmp/nautilus_out_dir.txt || true

OUT_DIR=$(cat /tmp/nautilus_out_dir.txt | head -1)
rm -f /tmp/nautilus_out_dir.txt

# Fallback: search target/debug/build if json parsing failed
if [ -z "$OUT_DIR" ] || [ ! -d "$OUT_DIR" ]; then
  echo -e "${YELLOW}JSON parse failed, searching target/debug/build...${NC}"
  OUT_DIR=$(find target/debug/build -type d -name "nautilus-serialization-*" -path "*/out" | head -1)
fi

if [ -z "$OUT_DIR" ] || [ ! -d "$OUT_DIR" ]; then
  echo -e "${RED}Error: Could not find OUT_DIR for nautilus-serialization${NC}"
  echo "Searched for: target/debug/build/nautilus-serialization-*/out"
  exit 1
fi

echo "Found OUT_DIR: $OUT_DIR"

# Copy generated files to the repo
echo -e "${YELLOW}Copying generated files to repository...${NC}"
mkdir -p crates/serialization/generated/capnp
cp -r "${OUT_DIR}/"* crates/serialization/generated/capnp/

# Format the generated files (requires nightly)
if rustup toolchain list | grep -q "nightly"; then
  echo -e "${YELLOW}Formatting generated files...${NC}"
  make format
else
  echo -e "${YELLOW}Warning: Nightly toolchain not found. Skipping formatting.${NC}"
  echo "Please run 'make format' manually after installing Rust nightly."
fi
# Show what was generated
echo -e "${GREEN}Successfully regenerated Cap'n Proto schemas!${NC}"
echo ""
echo "Generated files:"
find crates/serialization/generated/capnp -name "*.rs" | sort

# Check if there are any changes
if git diff --quiet crates/serialization/generated/capnp; then
  echo -e "${GREEN}No changes detected - schemas are up to date${NC}"
else
  echo -e "${YELLOW}Changes detected in generated files${NC}"
  echo "Review the changes with: git diff crates/serialization/generated/capnp"
fi
