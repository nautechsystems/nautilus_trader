#!/usr/bin/env bash
# Enforces documentation conventions:
#
# Rust (crates/**/*.rs):
# 1. `# Panics` on Result-returning functions with no panic tokens in body
# 2. `# Panics` sections that say "does not panic" (self-contradictory)
# 3. `# Errors` on functions that don't return Result/Option
#
# Suppress with `// panics-doc-ok` above the doc block for transitive panics.
# Suppress with `// errors-doc-ok` above the doc block for special cases.
#
# Markdown (docs/**/*.md):
# 4. Hyphen-split words in table rows (e.g., "configu- ration")
# 5. Soft hyphens (U+00AD)
# 6. Table lines ending with a trailing hyphen on a word fragment
# 7. Breakable hyphens in table prose (compound words need U+2011)

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

  # Breakable hyphens in table prose: lowercase-hyphen-letter (U+002D should be U+2011)
  # Strip backtick code spans and link targets before checking
  # shellcheck disable=SC2016
  while IFS= read -r match; do
    [[ -z "$match" ]] && continue
    echo -e "${RED}Error:${NC} Breakable hyphen in table row in ${md_file}:${match}"
    echo "  Use a non-breaking hyphen (U+2011) for compound words in tables"
    VIOLATIONS=$((VIOLATIONS + 1))
  done < <(sed 's/`[^`]*`//g; s/\]([^)]*)//g' "$md_file" | rg -n '^\|.*[a-z]-[a-zA-Z]' 2> /dev/null || true)

done < <(find docs -type f -name "*.md" 2> /dev/null || true)

if [ $VIOLATIONS -gt 0 ]; then
  echo -e "${RED}Found $VIOLATIONS documentation convention violation(s)${NC}"
  exit 1
fi

echo "✓ All documentation conventions are valid"
exit 0
