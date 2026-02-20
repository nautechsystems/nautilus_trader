#!/usr/bin/env bash
# Enforces Rust documentation conventions:
#
# 1. `# Panics` on Result-returning functions with no panic tokens in body
# 2. `# Panics` sections that say "does not panic" (self-contradictory)
# 3. `# Errors` on functions that don't return Result/Option
#
# Suppress with `// panics-doc-ok` above the doc block for transitive panics.
# Suppress with `// errors-doc-ok` above the doc block for special cases.

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

# =============================================================================
# Process each file that has either `# Panics` or `# Errors` docs
# =============================================================================

while IFS= read -r file; do
  [[ -z "$file" ]] && continue

  # Read file into array once (0-indexed, lines[0] = line 1)
  mapfile -t lines < "$file"
  total=${#lines[@]}

  # Collect line numbers for # Panics and # Errors in this file
  panics_lines=()
  errors_lines=()
  for ((i = 0; i < total; i++)); do
    case "${lines[i]}" in
      *'/// # Panics'*) panics_lines+=($((i + 1))) ;;
      *'/// # Errors'*) errors_lines+=($((i + 1))) ;;
    esac
  done

  # --- Check `# Panics` docs ---
  for line_num in "${panics_lines[@]}"; do
    idx=$((line_num - 1))

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
      lower="${lines[j],,}"
      if [[ "$lower" == *'does not panic'* ]] || [[ "$lower" == *'will never panic'* ]]; then
        # Find fn name for context
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
  done

  # --- Check `# Errors` docs ---
  for line_num in "${errors_lines[@]}"; do
    idx=$((line_num - 1))

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
  done

done < <(rg -l '/// # (Panics|Errors)' crates --type rust 2> /dev/null || true)

if [ $VIOLATIONS -gt 0 ]; then
  echo -e "${RED}Found $VIOLATIONS documentation convention violation(s)${NC}"
  exit 1
fi

echo "✓ All documentation conventions are valid"
exit 0
