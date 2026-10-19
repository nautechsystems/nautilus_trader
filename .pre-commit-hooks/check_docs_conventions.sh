#!/usr/bin/env bash
# Enforces documentation conventions:
#
# Rust (crates/**/*.rs):
# 1. `# Panics` on Result-returning functions with no panic tokens in body
# 2. `# Panics` sections that say "does not panic" (self-contradictory)
# 3. `# Errors` on functions that don't return Result/Option
# 4. Nonstandard Errors/Panics lead-ins and condition bullets without periods
#
# Cargo features (crates/**/Cargo.toml):
# 5. Every non-default feature is listed once, in alphabetical order, under
#    `Feature flags` in both README.md and the configured library source
#
# Suppress with `// panics-doc-ok` above the doc block for transitive panics.
# Suppress with `// errors-doc-ok` above the doc block for special cases.
#
# Markdown (docs/**/*.md):
# 6. Hyphen-split words in table rows (e.g., "configu- ration")
# 7. Soft hyphens (U+00AD)
# 8. Table lines ending with a trailing hyphen on a word fragment
#
# Documentation site URLs (tracked sources, excluding RELEASES.md):
# 9. `.html` suffix on a nautilustrader.io/docs URL, which the site does not serve

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

manifest_lib_path() {
  awk '
    /^[[:space:]]*\[lib\][[:space:]]*(#.*)?$/ {
      in_lib = 1
      next
    }
    in_lib && /^[[:space:]]*\[/ {
      exit
    }
    in_lib && /^[[:space:]]*path[[:space:]]*=/ {
      path = $0
      sub(/^[[:space:]]*path[[:space:]]*=[[:space:]]*/, "", path)
      sub(/[[:space:]]+#.*$/, "", path)
      quote = substr(path, 1, 1)
      single_quote = sprintf("%c", 39)
      if ((quote == "\"" || quote == single_quote) && substr(path, length(path), 1) == quote) {
        path = substr(path, 2, length(path) - 2)
      }
      print path
      found = 1
      exit
    }
    END {
      if (!found) {
        print "src/lib.rs"
      }
    }
  ' "$1"
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
doc_section_files=$(rg -l '^[[:space:]]*/// # (Panics|Errors)[[:space:]]*$' crates --type rust --sort path 2> /dev/null) || true

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

    # Line of the fn declaration within the 20 lines after the doc block, or 0
    function fn_line_after(i,    j) {
      while (i + 1 <= total && lines[i + 1] ~ /^[[:space:]]*\/\/\//) i++
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

    function check_section_style(file, i,    j, text, section, intro, intro_done, intro_line, bullet, bullet_line, has_bullets, fence, fn_line, return_type) {
      section = index(lines[i], "# Panics") > 0 ? "Panics" : "Errors"
      intro = ""
      intro_done = 0
      intro_line = i
      bullet = ""
      has_bullets = 0
      fence = ""
      for (j = i + 1; j <= total; j++) {
        if (lines[j] !~ /^[[:space:]]*\/\/\//) break
        text = doc_text_of(lines[j])
        if (fence != "") {
          if (substr(text, 1, length(fence)) == fence) fence = ""
          continue
        }
        if (text ~ /^#/) break
        if (fence_of(text) != "") {
          if (bullet != "" && bullet !~ /\.$/) print "bullet-period\t" bullet_line "\t" section "\t" file
          bullet = ""
          fence = fence_of(text)
          if (intro != "") intro_done = 1
          continue
        }
        if (text == "") {
          if (bullet != "" && bullet !~ /\.$/) print "bullet-period\t" bullet_line "\t" section "\t" file
          bullet = ""
          if (intro != "") intro_done = 1
          continue
        }
        if (text ~ /^[-*+][[:space:]]+/) {
          intro_done = 1
          has_bullets = 1
          if (bullet != "" && bullet !~ /\.$/) print "bullet-period\t" bullet_line "\t" section "\t" file
          bullet = text
          bullet_line = j
        } else if (!intro_done) {
          if (intro == "") intro_line = j
          intro = intro (intro == "" ? "" : " ") text
        } else if (bullet != "") {
          bullet = bullet " " text
        }
      }
      if (bullet != "" && bullet !~ /\.$/) print "bullet-period\t" bullet_line "\t" section "\t" file
      if (section == "Panics") {
        if (intro !~ /^Panics if([[:space:]]|:)/) print "lead-in\t" intro_line "\t" section "\t" file
      } else if (intro !~ /^Returns (an error|`None`) if([[:space:]]|:)/) {
        print "lead-in\t" intro_line "\t" section "\t" file
      } else {
        fn_line = fn_line_after(i)
        return_type = fn_line > 0 ? return_type_from(fn_line) : ""
        if ((return_type == "Option" && intro !~ /^Returns `None` if/) ||
            (return_type == "Result" && intro !~ /^Returns an error if/)) {
          print "return-lead-in\t" intro_line "\t" return_type "\t" file
        }
      }
      if ((has_bullets && intro != "Panics if:" && intro != "Returns an error if:" && intro != "Returns `None` if:") ||
          (!has_bullets && intro ~ /^(Panics|Returns (an error|`None`)) if:/)) {
        print "list-lead-in\t" intro_line "\t" section "\t" file
      }
    }

    function return_type_from(fn_line,    signature, j, char, generics, parameters, tail, name) {
      signature = signature_from(fn_line)
      if (!match(signature, /fn[[:space:]]+[a-zA-Z_][a-zA-Z0-9_]*/)) return ""
      signature = substr(signature, RSTART + RLENGTH)
      generics = 0
      parameters = 0
      tail = ""
      for (j = 1; j <= length(signature); j++) {
        char = substr(signature, j, 1)
        if (parameters > 0) {
          if (char == "(") parameters++
          else if (char == ")") {
            parameters--
            if (parameters == 0) {
              tail = substr(signature, j + 1)
              break
            }
          }
        } else if (char == "<") generics++
        else if (char == ">" && substr(signature, j - 1, 1) != "-") generics--
        else if (char == "(" && generics == 0) parameters = 1
      }
      if (tail !~ /^[[:space:]]*->[[:space:]]*[a-zA-Z_][a-zA-Z0-9_:]*/) return ""
      sub(/^[[:space:]]*->[[:space:]]*/, "", tail)
      match(tail, /^[a-zA-Z_][a-zA-Z0-9_:]*/)
      name = substr(tail, RSTART, RLENGTH)
      sub(/^.*::/, "", name)
      if (name == "Option") return "Option"
      if (name ~ /Result$/) return "Result"
      return ""
    }

    function doc_text_of(text) {
      sub(/^[[:space:]]*\/\/\/[[:space:]]?/, "", text)
      sub(/^[[:space:]]+/, "", text)
      sub(/[[:space:]]+$/, "", text)
      return text
    }

    function fence_of(text,    marker, fence) {
      if (text !~ /^```/ && text !~ /^~~~/) return ""
      marker = substr(text, 1, 1)
      fence = ""
      while (substr(text, length(fence) + 1, 1) == marker) fence = fence marker
      return fence
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

      doc_fence = ""
      for (i = 1; i <= total; i++) {
        if (lines[i] !~ /^[[:space:]]*\/\/\//) {
          doc_fence = ""
          continue
        }
        text = doc_text_of(lines[i])
        if (doc_fence != "") {
          if (substr(text, 1, length(doc_fence)) == doc_fence) doc_fence = ""
          continue
        }
        doc_fence = fence_of(text)
        if (doc_fence != "") continue
        if (lines[i] !~ /^[[:space:]]*\/\/\/ # (Panics|Errors)[[:space:]]*$/) continue
        check_section_style(file, i)
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
    lead-in)
      echo -e "${RED}Error:${NC} Nonstandard \`# ${fn_name}\` lead-in in $file:$line_num"
      if [[ "$fn_name" == "Panics" ]]; then
        echo '  Start with "Panics if"'
      else
        echo "  Start with \"Returns an error if\" or \"Returns \`None\` if\" for Option"
      fi
      echo
      ;;
    return-lead-in)
      echo -e "${RED}Error:${NC} Incorrect \`# Errors\` lead-in for $fn_name in $file:$line_num"
      if [[ "$fn_name" == "Option" ]]; then
        echo "  Start with \"Returns \`None\` if\""
      else
        echo '  Start with "Returns an error if"'
      fi
      echo
      ;;
    list-lead-in)
      echo -e "${RED}Error:${NC} Nonstandard \`# ${fn_name}\` condition list lead-in in $file:$line_num"
      echo '  Use the standard lead-in followed immediately by a colon and condition bullets'
      echo
      ;;
    bullet-period)
      echo -e "${RED}Error:${NC} Missing period on \`# ${fn_name}\` condition bullet in $file:$line_num"
      echo '  End each complete bullet, including wrapped continuations, with a period'
      echo
      ;;
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
  lib_path=$(manifest_lib_path "$manifest")
  check_feature_docs "$crate_dir/README.md" "## Feature flags" false "$expected"
  check_feature_docs "$crate_dir/$lib_path" "# Feature Flags" true "$expected"
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

# =============================================================================
# Documentation site URL shape
# =============================================================================

# nautilustrader.io serves docs pages as directory URLs; the `.html` form 404s.
# RELEASES.md is excluded because its entries record links as they were published, and
# this hook's own tests because their fixtures carry the dead form on purpose. Hidden
# paths are scanned so tracked files under .github/ and .pre-commit-hooks/ are covered,
# which makes excluding .git essential: a commit message naming the dead form lands in
# .git/COMMIT_EDITMSG and would block the next commit from a path no one can edit.
while IFS= read -r hit; do
  [[ -z "$hit" ]] && continue
  echo -e "${RED}Error:${NC} Dead \`.html\` docs URL in $hit"
  echo "  nautilustrader.io/docs serves directory URLs, so drop \`.html\`"
  echo
  VIOLATIONS=$((VIOLATIONS + 1))
done < <(rg -n --no-heading --hidden -g '!.git/**' -g '!RELEASES.md' -g '!.pre-commit-hooks/test_*.sh' \
  'nautilustrader\.io/docs/[^\s)>"]*\.html' . --sort path 2> /dev/null || true)

if [ $VIOLATIONS -gt 0 ]; then
  echo -e "${RED}Found $VIOLATIONS documentation convention violation(s)${NC}"
  exit 1
fi

echo "✓ All documentation conventions are valid"
exit 0
