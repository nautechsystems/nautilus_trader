#!/usr/bin/env bash
# Check that copyright years in headers match the current year

set -euo pipefail

CURRENT_YEAR=$(date -u +%Y)
FAILED=0

# Pattern to match: "Copyright (C) 2015-YYYY"
# For Python: #  Copyright (C) 2015-YYYY
# For Rust:   //  Copyright (C) 2015-YYYY

# Files to exclude from missing header warnings
is_excluded_from_header_check() {
  local file="$1"
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
      echo "ERROR: $file: Copyright year is $YEAR, expected >=$CURRENT_YEAR"
      FAILED=1
    fi
  fi
done < <(git grep -n -I -E "Copyright [(]C[)] 2015-[0-9]{4}" -- '*.rs' '*.py')

# Get list of files with copyright headers (sorted for comm)
git grep -l -I -F "Copyright (C)" -- '*.rs' '*.py' 2> /dev/null | sort > /tmp/files_with_headers.$$ || true

# Get all tracked files (sorted for comm)
git ls-files '*.rs' '*.py' | sort > /tmp/all_files.$$

# Find files without headers (in all_files but not in files_with_headers)
while IFS= read -r file; do
  if ! is_excluded_from_header_check "$file"; then
    echo "WARNING: $file: Missing copyright header"
  fi
done < <(comm -23 /tmp/all_files.$$ /tmp/files_with_headers.$$)

# Cleanup temp files
rm -f /tmp/files_with_headers.$$ /tmp/all_files.$$

if [[ $FAILED -eq 1 ]]; then
  echo ""
  echo "Fix: Update copyright headers to: Copyright (C) 2015-${CURRENT_YEAR} (or later)"
  exit 1
fi

echo "All copyright years are current or forward-dated"
exit 0
