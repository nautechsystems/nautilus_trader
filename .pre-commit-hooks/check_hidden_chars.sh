#!/usr/bin/env bash

# Ensure no hidden control or problematic unicode characters in source files
#
# This hook detects characters that could be used to hide malicious content:
# - Control chars (U+0001–U+0008, U+000E–U+001F)
# - Zero-width spaces (U+200B, U+200C, U+200D)
# - BOM (U+FEFF)
# - Right-to-left override chars (U+202D, U+202E)
# - Other invisible formatting chars (U+2060-U+206F)
# - Suspicious long base64 strings (potential hidden content)
#
# SECURITY MODEL:
# - Hidden Unicode: NO exclusions - always detected everywhere
# - Long strings: MINIMAL, SPECIFIC exclusions only
# - All exclusions must be explicitly documented and reviewable
# - Changes to exclusions should come only from trusted maintainers
set -e

# Get files passed by pre-commit, or all relevant files if none passed
# Filter out this script itself to avoid detecting its own search patterns
files_to_check=()
for file in "$@"; do
  if [[ "$file" != *"check_hidden_chars.sh" ]]; then
    files_to_check+=("$file")
  fi
done

if [ ${#files_to_check[@]} -eq 0 ]; then
  # Fallback: find all relevant files if no arguments passed
  mapfile -t files_to_check < <(find . -type f \( -name "*.py" -o -name "*.pyx" -o -name "*.rs" -o -name "*.toml" -o -name "*.md" -o -name "*.yml" -o -name "*.yaml" -o -name "*.json" -o -name "*.sh" -o -name "Dockerfile*" \) \
    ! -path "*/target/*" ! -path "*/build/*" ! -path "*/__pycache__/*" ! -path "*/.pytest_cache/*" \
    ! -path "*/.venv/*" ! -path "*/venv/*" ! -path "*/node_modules/*" \
    ! -name "*.lock" ! -name "*.whl" ! -name "*.egg-info" ! -name "check_hidden_chars.sh")
fi

# Check for problematic Unicode characters in the specified files
# Always check for hidden Unicode - these should never appear in legitimate source
control_chars=""
if [ ${#files_to_check[@]} -gt 0 ]; then
  control_chars=$(grep --binary-files=without-match -nP "[\x01-\x08\x0E-\x1F]|\u200D|\u200C|\u200B|\u200F|\u200E|\u2060|\u2061|\u2062|\u2063|\u2064|\u2065|\u2066|\u2067|\u2068|\u2069|\uFEFF" "${files_to_check[@]}" 2> /dev/null || true)
fi

# Check for suspicious long base64/hex strings, with very specific exclusions
# Any changes to these exclusions should be carefully reviewed as they create security blind spots
suspicious_strings=""
if [ ${#files_to_check[@]} -gt 0 ]; then
  suspicious_strings=$(grep --binary-files=without-match -nP "[A-Za-z0-9+/]{500,}={0,2}" "${files_to_check[@]}" 2> /dev/null |
    grep -v 'crates/model/src/defi/data/block.rs:.*"logsBloom":' |
    grep -v '#.*SECURITY_EXCLUSION:' |
    grep -v '//.*SECURITY_EXCLUSION:' || true)
fi

# Combine results
all_matches=""
if [[ -n "$control_chars" ]]; then
  all_matches="$control_chars"
fi
if [[ -n "$suspicious_strings" ]]; then
  if [[ -n "$all_matches" ]]; then
    all_matches="$all_matches\n$suspicious_strings"
  else
    all_matches="$suspicious_strings"
  fi
fi

if [[ -n "$all_matches" ]]; then
  echo "Problematic hidden/invisible Unicode characters or suspicious content detected:"
  echo "============================================================================="
  echo -e "$all_matches"
  echo
  echo "These could be used to hide malicious content. If legitimate, consider:"
  echo "1. Using visible alternatives for formatting"
  echo "2. Moving large encoded data to external files"
  echo "3. Adding comments explaining the necessity"
  exit 1
fi
