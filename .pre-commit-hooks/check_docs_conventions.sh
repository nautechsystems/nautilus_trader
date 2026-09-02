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

# Regex patterns (bash extended regex)
FN_RE='^[[:space:]]*(pub(\([^)]*\))?[[:space:]]+)?(async[[:space:]]+)?(unsafe[[:space:]]+)?(fn|const fn)[[:space:]]'
FN_NAME_RE='fn[[:space:]]+([a-zA-Z_][a-zA-Z0-9_]*)'
DOC_RE='^[[:space:]]*///'
PANIC_BODY_RE='\.(unwrap|expect)\(|panic!\(|assert!|assert_eq!|assert_ne!|unreachable!\(|todo!\(|unimplemented!\('

# Helper: read file into lines array (0-indexed, lines[0] = line 1)
read_file_lines() {
  if type mapfile &> /dev/null; then
    mapfile -t lines < "$1"
  else
    lines=()
    while IFS= read -r _line || [[ -n "$_line" ]]; do
      lines+=("$_line")
    done < "$1"
  fi
  total=${#lines[@]}
}

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
# Use rg to find all files and line numbers with `# Panics` or `# Errors`
# =============================================================================

current_file=""

while IFS=: read -r file line_num match; do
  [[ -z "$file" ]] && continue

  # Load file into array only when we encounter a new file
  if [[ "$file" != "$current_file" ]]; then
    current_file="$file"
    read_file_lines "$file"
  fi

  idx=$((line_num - 1))

  if [[ "$match" == *'# Panics'* ]]; then
    # --- Check `# Panics` docs ---

    # Check for suppression above the doc block
    suppressed=false
    j=$((idx - 1))
    while [[ $j -ge 0 ]]; do
      if [[ "${lines[j]}" =~ $DOC_RE ]]; then
        j=$((j - 1))
        continue
      fi
      if [[ "${lines[j]}" == *'panics-doc-ok'* ]]; then
        suppressed=true
      fi
      break
    done
    [[ "$suppressed" == true ]] && continue

    # Check for self-contradictory "does not panic" text
    contradictory=false
    for ((j = idx + 1; j <= idx + 4 && j < total; j++)); do
      lower="$(printf '%s' "${lines[j]}" | tr '[:upper:]' '[:lower:]')"
      if [[ "$lower" == *'does not panic'* ]] || [[ "$lower" == *'will never panic'* ]]; then
        fn_context="<unknown>"
        for ((k = idx + 1; k < total && k <= idx + 15; k++)); do
          if [[ "${lines[k]}" =~ $FN_NAME_RE ]]; then
            fn_context="${BASH_REMATCH[1]}"
            break
          fi
        done
        echo -e "${RED}Error:${NC} Self-contradictory \`# Panics\` doc on \`${fn_context}\` in $file:$line_num"
        echo "  Doc says function does not panic under a \`# Panics\` heading"
        echo "  Remove the \`# Panics\` section entirely"
        echo
        VIOLATIONS=$((VIOLATIONS + 1))
        contradictory=true
        break
      fi
    done
    [[ "$contradictory" == true ]] && continue

    # Find fn declaration
    fn_idx=""
    for ((j = idx + 1; j < total && j <= idx + 20; j++)); do
      if [[ "${lines[j]}" =~ $FN_RE ]]; then
        fn_idx=$j
        break
      fi
    done
    [[ -z "$fn_idx" ]] && continue

    # Extract fn name
    fn_name=""
    if [[ "${lines[fn_idx]}" =~ $FN_NAME_RE ]]; then
      fn_name="${BASH_REMATCH[1]}"
    fi

    # Build signature to check return type
    sig=""
    for ((j = fn_idx; j < total && j <= fn_idx + 40; j++)); do
      sig+="${lines[j]}"
      if [[ "${lines[j]}" == *'{'* ]]; then
        break
      fi
    done

    # Only check Result-returning functions
    if [[ ! "$sig" =~ -\>.*(Result|PyResult) ]]; then
      continue
    fi

    # Find function body boundaries via brace counting
    brace_count=0
    body_start=""
    body_end=""
    for ((j = fn_idx; j < total && j <= fn_idx + 500; j++)); do
      l="${lines[j]}"
      opens="${l//[^\{]/}"
      closes="${l//[^\}]/}"
      brace_count=$((brace_count + ${#opens} - ${#closes}))
      if [[ -z "$body_start" ]] && [[ ${#opens} -gt 0 ]]; then
        body_start=$j
      fi
      if [[ -n "$body_start" ]] && [[ $brace_count -le 0 ]]; then
        body_end=$j
        break
      fi
    done
    [[ -z "$body_end" ]] && continue

    # Check body for panic tokens
    has_panic=false
    for ((j = body_start; j <= body_end; j++)); do
      if [[ "${lines[j]}" =~ $PANIC_BODY_RE ]]; then
        has_panic=true
        break
      fi
    done

    if [[ "$has_panic" == false ]]; then
      echo -e "${RED}Error:${NC} False \`# Panics\` doc on \`${fn_name}\` in $file:$line_num"
      echo "  Function returns Result and contains no panic-inducing code"
      echo "  Remove the \`# Panics\` section, use \`# Errors\` instead,"
      echo "  or add \`// panics-doc-ok\` if the panic is in a called function"
      echo
      VIOLATIONS=$((VIOLATIONS + 1))
    fi

  else
    # --- Check `# Errors` docs ---

    # Check for suppression above the doc block
    suppressed=false
    j=$((idx - 1))
    while [[ $j -ge 0 ]]; do
      if [[ "${lines[j]}" =~ $DOC_RE ]]; then
        j=$((j - 1))
        continue
      fi
      if [[ "${lines[j]}" == *'errors-doc-ok'* ]]; then
        suppressed=true
      fi
      break
    done
    [[ "$suppressed" == true ]] && continue

    # Find fn declaration
    fn_idx=""
    for ((j = idx + 1; j < total && j <= idx + 20; j++)); do
      if [[ "${lines[j]}" =~ $FN_RE ]]; then
        fn_idx=$j
        break
      fi
    done
    [[ -z "$fn_idx" ]] && continue

    # Extract fn name
    fn_name=""
    if [[ "${lines[fn_idx]}" =~ $FN_NAME_RE ]]; then
      fn_name="${BASH_REMATCH[1]}"
    fi

    # Build signature to check return type
    sig=""
    for ((j = fn_idx; j < total && j <= fn_idx + 40; j++)); do
      sig+="${lines[j]}"
      if [[ "${lines[j]}" == *'{'* ]]; then
        break
      fi
    done

    if [[ ! "$sig" =~ -\>.*(Result|PyResult|Option) ]]; then
      echo -e "${RED}Error:${NC} False \`# Errors\` doc on \`${fn_name}\` in $file:$line_num"
      echo "  Function does not return Result or Option"
      echo "  Remove the \`# Errors\` section"
      echo
      VIOLATIONS=$((VIOLATIONS + 1))
    fi
  fi

done < <(rg -n '/// # (Panics|Errors)' crates --type rust --sort path 2> /dev/null || true)

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

while IFS= read -r md_file; do
  [[ -f "$md_file" ]] || continue

  # Hyphen-split words in table rows: "configu- ration"
  while IFS= read -r match; do
    [[ -z "$match" ]] && continue
    echo -e "${RED}Error:${NC} Possible word split in ${md_file}:${match}"
    VIOLATIONS=$((VIOLATIONS + 1))
  done < <(rg -n '^\|.*[a-z]- [a-z]' "$md_file" 2> /dev/null || true)

  # Soft hyphens (U+00AD)
  while IFS= read -r match; do
    [[ -z "$match" ]] && continue
    echo -e "${RED}Error:${NC} Soft hyphen (U+00AD) in ${md_file}:${match}"
    VIOLATIONS=$((VIOLATIONS + 1))
  done < <(rg -n '\x{00AD}' "$md_file" 2> /dev/null || true)

  # Table lines ending with a trailing hyphen on a word fragment
  while IFS= read -r match; do
    [[ -z "$match" ]] && continue
    echo -e "${RED}Error:${NC} Trailing hyphen at end of table line in ${md_file}:${match}"
    VIOLATIONS=$((VIOLATIONS + 1))
  done < <(rg -n '^\|.*[a-z]-\s*$' "$md_file" 2> /dev/null || true)

done < <(find docs -type f -name "*.md" 2> /dev/null || true)

if [ $VIOLATIONS -gt 0 ]; then
  echo -e "${RED}Found $VIOLATIONS documentation convention violation(s)${NC}"
  exit 1
fi

echo "✓ All documentation conventions are valid"
exit 0
