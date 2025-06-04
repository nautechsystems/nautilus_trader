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

# Exclude directories and file types that legitimately contain encoded data
exclude_dirs="--exclude-dir=.git --exclude-dir=target --exclude-dir=build --exclude-dir=__pycache__ --exclude-dir=.pytest_cache --exclude-dir=.venv --exclude-dir=venv --exclude-dir=node_modules"
exclude_files="--exclude=*.lock --exclude=*.whl --exclude=*.egg-info --exclude=check_hidden_chars.sh"

# Check for problematic Unicode characters in all source code
# Always check for hidden Unicode - these should never appear in legitimate source
control_chars=$(grep -R --binary-files=without-match -nP "[\x01-\x08\x0E-\x1F]|‍|‌|​|‏|‎|⁠|⁡|⁢|⁣|⁤|⁥|⁦|⁧|⁨|⁩|￿" $exclude_dirs $exclude_files --include="*.py" --include="*.pyx" --include="*.rs" --include="*.toml" --include="*.md" --include="*.txt" --exclude="*test*" --exclude="*Test*" . || true)

# Check for suspicious long base64/hex strings, with very specific exclusions
# Any changes to these exclusions should be carefully reviewed as they create security blind spots
suspicious_strings=$(grep -R --binary-files=without-match -nP "[A-Za-z0-9+/]{500,}={0,2}" $exclude_dirs --include="*.py" --include="*.pyx" --include="*.rs" --exclude="*test*" --exclude="*Test*" . | \
    grep -v 'crates/model/src/defi/block.rs:.*"logsBloom":' | \
    grep -v '#.*SECURITY_EXCLUSION:' | \
    grep -v '//.*SECURITY_EXCLUSION:' || true)

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
