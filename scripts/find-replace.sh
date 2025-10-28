#!/usr/bin/env bash
set -euo pipefail

# Usage information
usage() {
  cat << EOF
Usage: $(basename "$0") [OPTIONS] PATTERN REPLACEMENT

Find and replace text across files using ripgrep and sed.

Arguments:
    PATTERN        The pattern to search for
    REPLACEMENT    The text to replace with

Options:
    -d, --dry-run  Show what would be changed without making changes
    -h, --help     Show this help message

Examples:
    $(basename "$0") "old_string" "new_string"
    $(basename "$0") --dry-run "old_string" "new_string"
EOF
  exit 0
}

# Parse options
DRY_RUN=false
while [[ $# -gt 0 ]]; do
  case $1 in
    -h | --help)
      usage
      ;;
    -d | --dry-run)
      DRY_RUN=true
      shift
      ;;
    -*)
      echo "Error: Unknown option $1" >&2
      echo "Run with --help for usage information" >&2
      exit 1
      ;;
    *)
      break
      ;;
  esac
done

# Validate arguments
if [[ $# -ne 2 ]]; then
  echo "Error: Expected 2 arguments, got $#" >&2
  echo "Run with --help for usage information" >&2
  exit 1
fi

PATTERN="$1"
REPLACEMENT="$2"

# Check if ripgrep is available
if ! command -v rg > /dev/null 2>&1; then
  echo "Error: ripgrep (rg) is not installed or not in PATH" >&2
  echo "Install it from: https://github.com/BurntSushi/ripgrep" >&2
  exit 1
fi

# Create temp file for ripgrep output (portable across macOS and Linux)
tmp_file=$(mktemp "${TMPDIR:-/tmp}/find-replace.XXXXXX")
trap 'rm -f "$tmp_file"' EXIT

# Run ripgrep, keeping stderr on terminal for diagnostics
set +e
rg --files-with-matches --fixed-strings --null "$PATTERN" > "$tmp_file"
rg_exit=$?
set -e

# Ripgrep exit codes: 0=matches found, 1=no matches, 2+=error
if [[ $rg_exit -gt 1 ]]; then
  echo "Error: ripgrep failed with exit code $rg_exit" >&2
  exit 1
fi

# Read results from temp file (null-delimited for safe handling of filenames with spaces)
files=()
while IFS= read -r -d '' file; do
  files+=("$file")
done < "$tmp_file"

if [[ ${#files[@]} -eq 0 ]]; then
  echo "No files found containing pattern: $PATTERN"
  exit 0
fi

echo "Found ${#files[@]} file(s) containing pattern: $PATTERN"

if [[ "$DRY_RUN" == true ]]; then
  echo -e "\n[DRY RUN] Would replace in the following files:"
  printf '  %s\n' "${files[@]}"
  echo -e "\nPreview of changes:"
  for file in "${files[@]}"; do
    echo -e "\n=== $file ==="
    rg --line-number --fixed-strings --max-count 5 "$PATTERN" "$file" 2> /dev/null || true
  done
else
  # Detect OS for sed compatibility (macOS requires -i with extension, Linux doesn't)
  if [[ "$OSTYPE" == "darwin"* ]]; then
    SED_INPLACE=(-i '')
  else
    SED_INPLACE=(-i)
  fi

  # Escape all regex metacharacters for literal sed replacement
  escape_sed() {
    printf '%s' "$1" | sed 's/[][$^.*+?(){}|/\\&]/\\&/g'
  }

  ESCAPED_PATTERN=$(escape_sed "$PATTERN")
  ESCAPED_REPLACEMENT=$(escape_sed "$REPLACEMENT")

  # Perform replacement, iterating over array to handle filenames with spaces
  for file in "${files[@]}"; do
    sed "${SED_INPLACE[@]}" "s|${ESCAPED_PATTERN}|${ESCAPED_REPLACEMENT}|g" "$file"
  done

  echo -e "\n✓ Replacement complete in ${#files[@]} file(s)"
fi
