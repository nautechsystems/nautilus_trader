#!/usr/bin/env bash
# Check that copyright years in headers match the current year

set -euo pipefail

# Exit cleanly if ripgrep is not installed
if ! command -v rg &> /dev/null; then
  echo "WARNING: ripgrep not found, skipping copyright year checks"
  exit 0
fi

# Exit cleanly if bash version doesn't support mapfile (requires bash 4+)
if [[ "${BASH_VERSINFO[0]}" -lt 4 ]]; then
  echo "WARNING: bash 4+ required for copyright checks (current: $BASH_VERSION), skipping"
  exit 0
fi

CURRENT_YEAR=$(date +%Y)
FAILED=0

# Pattern to match: "Copyright (C) 2015-YYYY"
# For Python: #  Copyright (C) 2015-YYYY
# For Rust:   //  Copyright (C) 2015-YYYY

# Files to exclude from missing header warnings
is_excluded_from_header_check() {
  local file="$1"
  [[ "$file" == "build.py" ]] ||
    [[ "$file" == nautilus_trader/core/rust/* ]] ||
    [[ "$file" == */core/rust/* ]] ||
    [[ "$file" == examples/* ]] ||
    [[ "$file" == */examples/* ]]
}

echo "Checking copyright years (expected: 2015-${CURRENT_YEAR} or later)..."

# Use ripgrep to find all copyright lines with years (much faster than sed+grep loop)
# Format: filename:line_number:Copyright (C) 2015-YYYY
while IFS=: read -r file _ line_content; do
  # Extract year from pattern "2015-YYYY"
  if [[ "$line_content" =~ 2015-([0-9]{4}) ]]; then
    YEAR="${BASH_REMATCH[1]}"

    if [[ "$YEAR" -lt "$CURRENT_YEAR" ]]; then
      echo "❌ $file: Copyright year is $YEAR, expected >=$CURRENT_YEAR"
      FAILED=1
    fi
  fi
done < <(rg --line-number --no-heading "Copyright \(C\) 2015-[0-9]{4}" -g '*.rs' -g '*.py' -g '*.pyx' -g '*.pxd')

# Get list of files with copyright headers (sorted for comm)
rg --files-with-matches "Copyright \(C\)" -g '*.rs' -g '*.py' -g '*.pyx' -g '*.pxd' 2> /dev/null | sort > /tmp/files_with_headers.$$ || true

# Get all tracked files (sorted for comm)
git ls-files '*.rs' '*.py' '*.pyx' '*.pxd' | sort > /tmp/all_files.$$

# Find files without headers (in all_files but not in files_with_headers)
while IFS= read -r file; do
  if ! is_excluded_from_header_check "$file"; then
    echo "⚠️  $file: Missing copyright header"
  fi
done < <(comm -23 /tmp/all_files.$$ /tmp/files_with_headers.$$)

# Cleanup temp files
rm -f /tmp/files_with_headers.$$ /tmp/all_files.$$

if [[ $FAILED -eq 1 ]]; then
  echo ""
  echo "Fix: Update copyright headers to: Copyright (C) 2015-${CURRENT_YEAR} (or later)"
  exit 1
fi

echo "✓ All copyright years are current or forward-dated"
exit 0
