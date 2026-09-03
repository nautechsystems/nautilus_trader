#!/usr/bin/env bash
# Enforces documentation conventions:
#
# Rust (crates/**/*.rs):
# 1. `# Panics` on Result-returning functions with no panic tokens in body
# 2. `# Panics` sections that say "does not panic" (self-contradictory)
# 3. `# Errors` on functions that don't return Result/Option
#
# Cargo features (crates/**/Cargo.toml):
# 4. Every non-default feature is listed once, in alphabetical order, under
#    `Feature flags` in both README.md and src/lib.rs
#
# Suppress with `// panics-doc-ok` above the doc block for transitive panics.
# Suppress with `// errors-doc-ok` above the doc block for special cases.
#
# Markdown (docs/**/*.md):
# 5. Hyphen-split words in table rows (e.g., "configu- ration")
# 6. Soft hyphens (U+00AD)
# 7. Table lines ending with a trailing hyphen on a word fragment

set -euo pipefail

# Exit cleanly if ripgrep is not installed
if ! command -v rg &> /dev/null; then
  echo "WARNING: ripgrep not found, skipping docs convention checks"
  exit 0
fi

# Color output
RED='\033[0;31m'
NC='\033[0m' # No Color

VIOLATIONS=0

manifest_feature_names() {
  awk '
    /^[[:space:]]*\[features\][[:space:]]*(#.*)?$/ {
      in_features = 1
      next
    }
    in_features && /^[[:space:]]*\[/ {
      exit
    }
    in_features {
      feature = $0
      sub(/^[[:space:]]*/, "", feature)
      single_quote = sprintf("%c", 39)
      is_feature = feature ~ /^[A-Za-z0-9_-]+[[:space:]]*=/
      is_feature = is_feature || feature ~ /^"[^"]+"[[:space:]]*=/
      is_feature = is_feature || feature ~ ("^" single_quote "[^" single_quote "]+" single_quote "[[:space:]]*=")
      if (!is_feature) {
        next
      }
      sub(/[[:space:]]*=.*$/, "", feature)
      quote = substr(feature, 1, 1)
      if ((quote == "\"" || quote == single_quote) && substr(feature, length(feature), 1) == quote) {
        feature = substr(feature, 2, length(feature) - 2)
      }
      if (feature != "default") {
        print feature
      }
    }
  ' "$1" | LC_ALL=C sort
}

documented_feature_names() {
  local file="$1"
  local heading="$2"
  local rustdoc="$3"

  awk -v heading="$heading" -v rustdoc="$rustdoc" '
    {
      line = $0
      if (rustdoc == "true") {
        sub(/^[[:space:]]*\/\/![[:space:]]?/, "", line)
      }
      if (line == heading) {
        found = 1
        in_section = 1
        next
      }
      if (in_section && line ~ /^#/) {
        exit
      }
      if (in_section && line ~ /^-[[:space:]]+`[^`]+`/) {
        sub(/^-[[:space:]]+`/, "", line)
        sub(/`.*/, "", line)
        print line
      }
    }
    END {
      if (!found) {
        exit 2
      }
    }
  ' "$file"
}

check_feature_docs() {
  local file="$1"
  local heading="$2"
  local rustdoc="$3"
  local expected="$4"
  local actual
  local expected_display
  local actual_display

  if [[ ! -f "$file" ]]; then
    echo -e "${RED}Error:${NC} Missing feature flag documentation file $file"
    VIOLATIONS=$((VIOLATIONS + 1))
    return
  fi

  if ! actual=$(documented_feature_names "$file" "$heading" "$rustdoc"); then
    echo -e "${RED}Error:${NC} Missing \`$heading\` section in $file"
    VIOLATIONS=$((VIOLATIONS + 1))
    return
  fi

  if [[ "$actual" == "$expected" ]]; then
    return
  fi

  expected_display=$(printf '%s\n' "$expected" | paste -sd ' ' -)
  if [[ -n "$actual" ]]; then
    actual_display=$(printf '%s\n' "$actual" | paste -sd ' ' -)
  else
    actual_display="<none>"
  fi

  echo -e "${RED}Error:${NC} Feature flag list mismatch in $file"
  echo "  Expected: $expected_display"
  echo "  Found:    $actual_display"
  echo "  List every non-default [features] key once under \`$heading\` in alphabetical order"
  echo
  VIOLATIONS=$((VIOLATIONS + 1))
}

# =============================================================================
# Rust doc sections: `# Panics` and `# Errors` (crates/**/*.rs)
# =============================================================================

# One awk pass reads every file with a matching heading and reports each
# violation as a kind, line, function name, and path record; the path comes
# last so a tab in it cannot split the fields.
doc_section_files=$(rg -l '/// # (Panics|Errors)' crates --type rust --sort path 2> /dev/null) || true

doc_section_output=""
if [[ -n "$doc_section_files" ]]; then
  doc_section_output=$(LC_ALL=C awk '
    # Name from the first `fn <name>` on the line, or empty
    function fn_name_of(text) {
      if (!match(text, /fn[[:space:]]+[a-zA-Z_][a-zA-Z0-9_]*/)) return ""
      text = substr(text, RSTART, RLENGTH)
      sub(/^fn[[:space:]]+/, "", text)
      return text
    }

    # Line of the fn declaration within the 20 lines after the heading, or 0
    function fn_line_after(i,    j) {
      for (j = i + 1; j <= i + 20 && j <= total; j++) {
        if (lines[j] ~ /^[[:space:]]*(pub(\([^)]*\))?[[:space:]]+)?(async[[:space:]]+)?(unsafe[[:space:]]+)?(fn|const fn)[[:space:]]/) return j
      }
      return 0
    }

    # Declaration text up to and including the first line carrying `{`
    function signature_from(fn_line,    j, signature) {
      signature = ""
      for (j = fn_line; j <= fn_line + 40 && j <= total; j++) {
        signature = signature lines[j]
        if (index(lines[j], "{") > 0) break
      }
      return signature
    }

    # Whether the first non-doc line above the heading carries `marker`
    function is_suppressed(i, marker,    j) {
      for (j = i - 1; j >= 1; j--) {
        if (lines[j] ~ /^[[:space:]]*\/\/\//) continue
        return index(lines[j], marker) > 0
      }
      return 0
    }

    # Function name for a "does not panic" claim under the heading, or empty
    function contradiction_fn(i,    j, k, lower, name) {
      for (j = i + 1; j <= i + 4 && j <= total; j++) {
        lower = tolower(lines[j])
        if (index(lower, "does not panic") == 0 && index(lower, "will never panic") == 0) continue
        for (k = i + 1; k <= i + 15 && k <= total; k++) {
          name = fn_name_of(lines[k])
          if (name != "") return name
        }
        return "<unknown>"
      }
      return ""
    }

    # Whether the brace-delimited body from `fn_line` has no panic token
    function body_lacks_panic(fn_line,    j, depth, opens, closes, body_start, body_end, parts) {
      depth = 0
      body_start = 0
      body_end = 0
      for (j = fn_line; j <= fn_line + 500 && j <= total; j++) {
        opens = split(lines[j], parts, "{") - 1
        closes = split(lines[j], parts, "}") - 1
        depth += opens - closes
        if (body_start == 0 && opens > 0) body_start = j
        if (body_start > 0 && depth <= 0) {
          body_end = j
          break
        }
      }
      if (body_end == 0) return 0
      for (j = body_start; j <= body_end; j++) {
        if (lines[j] ~ /\.(unwrap|expect)\(|panic!\(|assert!|assert_eq!|assert_ne!|unreachable!\(|todo!\(|unimplemented!\(/) return 0
      }
      return 1
    }

    function check_panics(file, i,    name, fn_line) {
      if (is_suppressed(i, "panics-doc-ok")) return
      name = contradiction_fn(i)
      if (name != "") {
        print "contradictory\t" i "\t" name "\t" file
        return
      }
      fn_line = fn_line_after(i)
      if (fn_line == 0) return
      if (signature_from(fn_line) !~ /->.*(Result|PyResult)/) return
      if (body_lacks_panic(fn_line)) print "panics\t" i "\t" fn_name_of(lines[fn_line]) "\t" file
    }

    function check_errors(file, i,    fn_line) {
      if (is_suppressed(i, "errors-doc-ok")) return
      fn_line = fn_line_after(i)
      if (fn_line == 0) return
      if (signature_from(fn_line) !~ /->.*(Result|PyResult|Option)/) print "errors\t" i "\t" fn_name_of(lines[fn_line]) "\t" file
    }

    {
      file = $0
      total = 0
      while ((status = (getline line < file)) > 0) lines[++total] = line
      close(file)
      if (status < 0) {
        print "ERROR: cannot read " file > "/dev/stderr"
        exit 1
      }

      for (i = 1; i <= total; i++) {
        if (lines[i] !~ /\/\/\/ # (Panics|Errors)/) continue
        if (index(lines[i], "# Panics") > 0) check_panics(file, i)
        else check_errors(file, i)
      }
      split("", lines)
    }
  ' <<< "$doc_section_files")
fi

while IFS=$'\t' read -r kind line_num fn_name file; do
  [[ -z "$kind" ]] && continue

  case "$kind" in
    contradictory)
      echo -e "${RED}Error:${NC} Self-contradictory \`# Panics\` doc on \`${fn_name}\` in $file:$line_num"
      echo "  Doc says function does not panic under a \`# Panics\` heading"
      echo "  Remove the \`# Panics\` section entirely"
      echo
      ;;
    panics)
      echo -e "${RED}Error:${NC} False \`# Panics\` doc on \`${fn_name}\` in $file:$line_num"
      echo "  Function returns Result and contains no panic-inducing code"
      echo "  Remove the \`# Panics\` section, use \`# Errors\` instead,"
      echo "  or add \`// panics-doc-ok\` if the panic is in a called function"
      echo
      ;;
    errors)
      echo -e "${RED}Error:${NC} False \`# Errors\` doc on \`${fn_name}\` in $file:$line_num"
      echo "  Function does not return Result or Option"
      echo "  Remove the \`# Errors\` section"
      echo
      ;;
  esac
  VIOLATIONS=$((VIOLATIONS + 1))
done <<< "$doc_section_output"

while IFS= read -r manifest; do
  expected=$(manifest_feature_names "$manifest")
  [[ -z "$expected" ]] && continue

  crate_dir=${manifest%/Cargo.toml}
  check_feature_docs "$crate_dir/README.md" "## Feature flags" false "$expected"
  check_feature_docs "$crate_dir/src/lib.rs" "# Feature Flags" true "$expected"
done < <(rg --files crates -g Cargo.toml 2> /dev/null | LC_ALL=C sort)

# =============================================================================
# Markdown table checks (docs/**/*.md)
# =============================================================================

# One ripgrep pass per rule; each hit is reported as file:line:content. Hidden
# files are included because the replaced find walk scanned them.
report_markdown_hits() {
  local message="$1"
  local pattern="$2"
  local hit

  while IFS= read -r hit; do
    [[ -z "$hit" ]] && continue
    echo -e "${RED}Error:${NC} $message in $hit"
    VIOLATIONS=$((VIOLATIONS + 1))
  done < <(rg -n --no-heading --hidden "$pattern" docs -g '*.md' --sort path 2> /dev/null || true)
}

# Hyphen-split words in table rows: "configu- ration"
report_markdown_hits "Possible word split" '^\|.*[a-z]- [a-z]'

# Soft hyphens (U+00AD)
report_markdown_hits "Soft hyphen (U+00AD)" '\x{00AD}'

# Table lines ending with a trailing hyphen on a word fragment
report_markdown_hits "Trailing hyphen at end of table line" '^\|.*[a-z]-\s*$'

if [ $VIOLATIONS -gt 0 ]; then
  echo -e "${RED}Found $VIOLATIONS documentation convention violation(s)${NC}"
  exit 1
fi

echo "✓ All documentation conventions are valid"
exit 0
