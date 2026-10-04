#!/usr/bin/env bash
# Check that copyright years in headers match the current year

set -euo pipefail

COPYRIGHT_TMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/nautilus-copyright.XXXXXX")
trap 'rm -rf "$COPYRIGHT_TMP_DIR"' EXIT

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
git grep -n -I -E "Copyright [(]C[)] 2015-[0-9]{4}" -- '*.rs' '*.py' \
  > "$COPYRIGHT_TMP_DIR/year_headers" || {
  status=$?
  [[ $status -eq 1 ]] || exit "$status"
}
while IFS=: read -r file _ line_content; do
  # Extract year from pattern "2015-YYYY"
  if [[ "$line_content" =~ 2015-([0-9]{4}) ]]; then
    YEAR="${BASH_REMATCH[1]}"

    if [[ "$YEAR" -lt "$CURRENT_YEAR" ]]; then
      echo "ERROR: $file: Copyright year is $YEAR, expected >=$CURRENT_YEAR"
      FAILED=1
    fi
  fi
done < "$COPYRIGHT_TMP_DIR/year_headers"

# Get list of files with copyright headers (sorted for comm)
{ git grep -l -I -F "Copyright (C)" -- '*.rs' '*.py' || {
  status=$?
  [[ $status -eq 1 ]] || exit "$status"
}; } |
  sort > "$COPYRIGHT_TMP_DIR/files_with_headers"

# Get all tracked files (sorted for comm)
git ls-files '*.rs' '*.py' | sort > "$COPYRIGHT_TMP_DIR/all_files"

# Find files without headers (in all_files but not in files_with_headers)
comm -23 "$COPYRIGHT_TMP_DIR/all_files" "$COPYRIGHT_TMP_DIR/files_with_headers" \
  > "$COPYRIGHT_TMP_DIR/missing_headers"
while IFS= read -r file; do
  if ! is_excluded_from_header_check "$file"; then
    echo "WARNING: $file: Missing copyright header"
  fi
done < "$COPYRIGHT_TMP_DIR/missing_headers"

if [[ $FAILED -eq 1 ]]; then
  echo ""
  echo "Fix: Update copyright headers to: Copyright (C) 2015-${CURRENT_YEAR} (or later)"
  exit 1
fi

echo "All copyright years are current or forward-dated"
exit 0
