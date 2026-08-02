#!/usr/bin/env bash

set -euo pipefail

if ! command -v perl &> /dev/null; then
  echo "ERROR: perl is required for Unicode typography checks"
  exit 1
fi

is_excluded() {
  local file="${1#./}"

  case "$file" in
    CLA.md | RELEASES.md | patches/*)
      return 0
      ;;
    */resources/*.csv | */resources/*.json | */resources/*.jsonl | */resources/*.txt | */resources/*.xml)
      return 0
      ;;
    */test_data/*.csv | */test_data/*.json | */test_data/*.jsonl | */test_data/*.txt | */test_data/*.xml)
      return 0
      ;;
  esac

  return 1
}

files_to_check=()

for file in "$@"; do
  if [[ ! -r "$file" ]] || is_excluded "$file"; then
    continue
  fi

  files_to_check+=("$file")
done

if [[ ${#files_to_check[@]} -eq 0 ]]; then
  echo "Unicode typography conventions are valid"
  exit 0
fi

matches=$(perl -Mutf8 -CSDA -ne '
  our %details;
  BEGIN {
    %details = (
      "\x{2013}" => ["U+2013 EN DASH", "ASCII hyphen (-)"],
      "\x{2014}" => ["U+2014 EM DASH", "ASCII hyphen (-)"],
      "\x{2018}" => ["U+2018 LEFT SINGLE QUOTATION MARK", "ASCII apostrophe (\x27)"],
      "\x{2019}" => ["U+2019 RIGHT SINGLE QUOTATION MARK", "ASCII apostrophe (\x27)"],
      "\x{201C}" => ["U+201C LEFT DOUBLE QUOTATION MARK", "ASCII double quote (\x22)"],
      "\x{201D}" => ["U+201D RIGHT DOUBLE QUOTATION MARK", "ASCII double quote (\x22)"],
      "\x{2705}" => ["U+2705 WHITE HEAVY CHECK MARK", "U+2713 CHECK MARK or ASCII text"],
      "\x{274C}" => ["U+274C CROSS MARK", "U+2717 BALLOT X or ASCII text"],
    );
  }

  unless (index($_, "unicode-typography: allow") >= 0) {
    my %seen;

    while (/([\x{2013}\x{2014}\x{2018}\x{2019}\x{201C}\x{201D}\x{2705}\x{274C}])/g) {
      my $char = $1;
      next if $seen{$char}++;

      my ($name, $replacement) = @{$details{$char}};
      print "$ARGV:$.: $name; use $replacement\n";
    }
  }

  close ARGV if eof;
' "${files_to_check[@]}")

if [[ -n "$matches" ]]; then
  printf '%s\n' "$matches"
  echo
  echo "Found Unicode typography violations"
  echo "Use 'unicode-typography: allow' only on intentional Unicode fixture lines"
  exit 1
fi

echo "Unicode typography conventions are valid"
