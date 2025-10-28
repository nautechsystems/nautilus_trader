#!/bin/bash
# Check for non-Latin script characters (CJK, Cyrillic, Arabic, etc.) in source files
# Uses perl for cross-platform compatibility (works on macOS and Linux)

set -e

# Check if perl is available
if ! command -v perl &> /dev/null; then
  echo "WARNING: perl not found, skipping non-Latin text check"
  exit 0
fi

exit_code=0

for file in "$@"; do
  # Skip if file doesn't exist or isn't readable
  if [[ ! -r "$file" ]]; then
    continue
  fi

  # Use perl to find specific non-English scripts (CJK, Cyrillic, Arabic)
  # This is a targeted blocklist rather than comprehensive Latin-only check
  # The -C flag enables UTF-8 handling
  # Highlights the offending character with >>> marker <<<
  matches=$(perl -C -ne 'if (/([\x{4E00}-\x{9FFF}\x{3040}-\x{30FF}\x{AC00}-\x{D7AF}\x{0400}-\x{04FF}\x{0600}-\x{06FF}])/) {
        my $char = $1;
        my $marked = $_;
        $marked =~ s/(\Q$char\E)/>>>$1<<</g;
        print "$.:$marked";
    }' "$file" 2> /dev/null || true)

  if [[ -n "$matches" ]]; then
    echo "ERROR: $file: Contains non-Latin characters (standardize to English)"
    echo "$matches" | head -5
    line_count=$(echo "$matches" | wc -l)
    if [[ $line_count -gt 5 ]]; then
      echo "   ... and $((line_count - 5)) more lines"
    fi
    exit_code=1
  fi
done

exit $exit_code
