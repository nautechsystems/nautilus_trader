#!/usr/bin/env bash
# uv has no native "block third-party sdist builds but not the local project"
# setting. Keep each `no-build-package` list aligned with its uv.lock so a new
# third-party package cannot silently fall back to a source build.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "$REPO_ROOT"

pairs=()
while (($# > 0)); do
  case "$1" in
    --pair)
      (($# >= 2)) || {
        echo "--pair requires LOCK:MANIFEST" >&2
        exit 2
      }
      pairs+=("$2")
      shift 2
      ;;
    -h | --help)
      echo "Usage: scripts/check-no-build-packages.sh [--pair LOCK:MANIFEST]..."
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

for tool in awk comm git grep sed sort tr uniq; do
  command -v "$tool" > /dev/null || {
    echo "Required tool not on PATH: $tool" >&2
    exit 2
  }
done

locked_third_party() {
  awk '
    function flush() {
      if (in_pkg && have_source && !is_local && name != "") print name
    }
    /^\[\[package\]\]/ {
      flush()
      in_pkg=1; name=""; have_source=0; is_local=0
      next
    }
    /^\[/ {
      flush()
      in_pkg=0; name=""; have_source=0; is_local=0
      next
    }
    in_pkg && /^name = "/ {
      n=$0
      sub(/^name = "/, "", n)
      sub(/"$/, "", n)
      name=n
    }
    in_pkg && /^source = / {
      have_source=1
      if ($0 ~ /^source = \{[[:space:]]*(editable|virtual|directory|path)[[:space:]]*=/) is_local=1
    }
    END { flush() }
  ' "$1" | LC_ALL=C sort -u
}

declared_packages() {
  awk '
    /^no-build-package = \[/ { in_list=1; next }
    in_list && /^\]/ { in_list=0; next }
    in_list && /^[[:space:]]*"/ {
      line=$0
      sub(/^[[:space:]]*"/, "", line)
      sub(/",?[[:space:]]*$/, "", line)
      print line
    }
  ' "$1"
}

if ((${#pairs[@]} == 0)); then
  for tool in mktemp rm; do
    command -v "$tool" > /dev/null || {
      echo "Required tool not on PATH: $tool" >&2
      exit 2
    }
  done
  discovery_root=$(mktemp -d "${TMPDIR:-/tmp}/nautilus-no-build-discovery.XXXXXX")
  trap 'rm -rf "$discovery_root"' EXIT
  git ls-files -z > "${discovery_root}/tracked"
  while IFS= read -r -d '' lock; do
    [[ "$lock" == "uv.lock" || "$lock" == */uv.lock ]] || continue
    manifest="$(dirname "$lock")/pyproject.toml"
    if [[ "$manifest" == "./pyproject.toml" ]]; then
      manifest=pyproject.toml
    fi
    [[ -f "$manifest" ]] || continue
    if grep -q '^[[:space:]]*no-build-package[[:space:]]*=' "$manifest"; then
      pairs+=("${lock}:${manifest}")
    else
      status=$?
      [[ "$status" == 1 ]] || exit "$status"
    fi
  done < "${discovery_root}/tracked"
fi

if ((${#pairs[@]} == 0)); then
  echo "No tracked uv.lock has a no-build-package policy."
  exit 0
fi

failures=0

validate_repo_path() {
  local path=$1 component current=""
  if [[ -z "$path" || "$path" == /* || "$path" == *\\* || "$path" == *$'\n'* ]]; then
    return 1
  fi
  IFS='/' read -r -a components <<< "$path"
  for component in "${components[@]}"; do
    if [[ -z "$component" || "$component" == "." || "$component" == ".." ]]; then
      return 1
    fi
    current=${current:+${current}/}${component}
    [[ ! -L "$current" ]] || return 1
  done
}

for pair in "${pairs[@]}"; do
  if [[ "$pair" != *:* || "${pair#*:}" == *:* ]]; then
    echo "ERROR: pair must be LOCK:MANIFEST: $pair" >&2
    exit 2
  fi
  lock="${pair%:*}"
  manifest="${pair#*:}"

  if ! validate_repo_path "$lock" || ! validate_repo_path "$manifest" ||
    [[ ! -f "$lock" || ! -f "$manifest" ]]; then
    echo "ERROR: missing $lock or $manifest" >&2
    exit 2
  fi

  locked=$(locked_third_party "$lock")
  declared_raw=$(declared_packages "$manifest")
  declared_ordered=$(printf '%s\n' "$declared_raw" | LC_ALL=C sort)
  declared_sorted=$(printf '%s\n' "$declared_ordered" | LC_ALL=C uniq)

  missing=$(LC_ALL=C comm -23 <(printf '%s\n' "$locked") <(printf '%s\n' "$declared_sorted"))
  stale=$(LC_ALL=C comm -13 <(printf '%s\n' "$locked") <(printf '%s\n' "$declared_sorted"))
  duplicates=$(printf '%s\n' "$declared_ordered" | LC_ALL=C uniq -d)
  out_of_order=""
  if [[ "$declared_raw" != "$declared_ordered" ]]; then
    out_of_order="yes"
  fi

  if [[ -z "$missing" && -z "$stale" && -z "$duplicates" && -z "$out_of_order" ]]; then
    count_status=0
    count=$(grep -c . <<< "$locked") || count_status=$?
    if ((count_status > 1)); then
      exit "$count_status"
    fi
    echo "OK  ${manifest}: ${count} packages, in sync with ${lock}"
    continue
  fi

  failures=$((failures + 1))
  echo "FAIL ${manifest}: out of sync with ${lock}"

  if [[ -n "$missing" ]]; then
    count=$(printf '%s\n' "$missing" | grep -c .)
    echo "  Missing from no-build-package (${count}):"
    printf '%s\n' "$missing" | sed 's/^/    + /'
  fi
  if [[ -n "$stale" ]]; then
    count=$(printf '%s\n' "$stale" | grep -c .)
    echo "  Listed in no-build-package but not in lock (${count}):"
    printf '%s\n' "$stale" | sed 's/^/    - /'
  fi
  if [[ -n "$duplicates" ]]; then
    duplicate_line=$(printf '%s' "$duplicates" | tr '\n' ' ')
    echo "  Duplicate entries: ${duplicate_line}"
  fi
  if [[ -n "$out_of_order" ]]; then
    echo "  Entries are not sorted alphabetically."
  fi
done

if ((failures > 0)); then
  echo >&2
  echo "Update no-build-package in each failing manifest to match its uv.lock." >&2
  exit 1
fi
