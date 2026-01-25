#!/usr/bin/env bash
# Enforces testing conventions:
# 1. Rust: Prefer #[rstest] over #[test] for consistency and parametrization support

set -euo pipefail

# Color output
RED='\033[0;31m'
YELLOW='\033[0;33m'
NC='\033[0m' # No Color

# Track if we found violations
VIOLATIONS=0

################################################################################
# Check for ripgrep availability
################################################################################

if ! command -v rg &> /dev/null; then
  echo -e "${YELLOW}WARNING: ripgrep (rg) not found, skipping testing convention checks${NC}"
  echo "Install ripgrep to enable this check: https://github.com/BurntSushi/ripgrep"
  exit 0
fi

################################################################################
# Check Rust files for #[test] instead of #[rstest]
################################################################################

echo "Checking Rust testing conventions..."

# Create temporary files to store search results
rust_results=$(mktemp)
aaa_results=$(mktemp)
trap 'rm -f "$rust_results" "$aaa_results"' EXIT

# Search for #[test] attribute in Rust files
# We want to find standalone #[test], not #[tokio::test] or #[rstest]
# Pattern: lines with #[test] but not #[test(...)] or #[tokio::test] or followed by #[rstest]
rg -n '^\s*#\[test\]' crates --type rust 2> /dev/null > "$rust_results" || true

while IFS=: read -r file line_num line_content; do
  # Skip empty lines
  [[ -z "$file" ]] && continue

  # Trim leading whitespace from line for display
  trimmed_line="${line_content#"${line_content%%[![:space:]]*}"}"

  echo -e "${RED}Error:${NC} Found #[test] instead of #[rstest] in $file:$line_num"
  echo "  Found: $trimmed_line"
  echo "  Expected: #[rstest]"
  echo "  Reason: Use #[rstest] for consistency and parametrization support"
  echo
  VIOLATIONS=$((VIOLATIONS + 1))
done < "$rust_results"

################################################################################
# Check Rust files for AAA-style comments (Arrange/Act/Assert)
# These are Python conventions and should not be used in Rust tests
################################################################################

echo "Checking for AAA-style comments in Rust tests..."

# Search for // Arrange, // Act, // Assert comments (standalone or with trailing content)
# Pattern: lines starting with whitespace, then // followed by Arrange, Act, or Assert
# We look for the standalone markers, not comments that happen to contain these words in context
rg -n '^\s*//\s*(Arrange|Act|Assert)\s*($|:|\s*-)' crates --type rust 2> /dev/null > "$aaa_results" || true

while IFS=: read -r file line_num line_content; do
  # Skip empty lines
  [[ -z "$file" ]] && continue

  # Trim leading whitespace from line for display
  trimmed_line="${line_content#"${line_content%%[![:space:]]*}"}"

  echo -e "${RED}Error:${NC} Found AAA-style comment in $file:$line_num"
  echo "  Found: $trimmed_line"
  echo "  Reason: Arrange/Act/Assert comments are a Python convention, not used in Rust tests"
  echo
  VIOLATIONS=$((VIOLATIONS + 1))
done < "$aaa_results"

################################################################################
# Report results
################################################################################

if [ $VIOLATIONS -gt 0 ]; then
  echo -e "${RED}Found $VIOLATIONS testing convention violation(s)${NC}"
  echo
  echo "Convention:"
  echo "  - Rust: Use #[rstest] instead of #[test] for consistency"
  echo "  - #[tokio::test] is acceptable for async tests without parametrization"
  echo "  - Do not use // Arrange / // Act / // Assert comments in Rust tests (Python convention)"
  echo
  echo "To fix:"
  echo "  - Replace #[test] with #[rstest] in your test functions"
  echo "  - Remove AAA-style comments or convert them to descriptive comments"
  exit 1
fi

echo "✓ All testing conventions are valid"
exit 0
