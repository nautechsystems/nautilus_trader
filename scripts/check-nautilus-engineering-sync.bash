#!/usr/bin/env bash
set -euo pipefail

LOCK_FILE=.nautilus-engineering.lock
PROCESS_LOCK_FILE=.nautilus-engineering.sync-lock
STAGED=false

while (($# > 0)); do
  case "$1" in
    --lock)
      (($# >= 2)) || {
        echo "--lock requires a value" >&2
        exit 2
      }
      LOCK_FILE=$2
      shift 2
      ;;
    --staged)
      STAGED=true
      shift
      ;;
    -h | --help)
      echo "Usage: check-nautilus-engineering-sync.bash [--staged] [--lock PATH]"
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

for tool in awk git mktemp tr uname; do
  command -v "$tool" > /dev/null || {
    echo "Required tool not on PATH: $tool" >&2
    exit 2
  }
done

CHECK_WORKTREE_MODE=true
case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*) CHECK_WORKTREE_MODE=false ;;
esac

if command -v sha256sum > /dev/null 2>&1; then
  HASH_COMMAND=sha256sum
elif command -v shasum > /dev/null 2>&1; then
  HASH_COMMAND=shasum
else
  echo "Neither sha256sum nor shasum is available" >&2
  exit 2
fi

validate_path() {
  local path=$1 component component_fold component_stem sanitized
  local -a components
  if [[ -z "$path" || "$path" == /* || "$path" == *\\* || "$path" == *'|'* ||
    "$path" == *':'* || "$path" == *'"'* ||
    "$path" == *$'\n'* || "$path" == *'//'* || "$path" == */ ]]; then
    return 1
  fi
  sanitized=$(printf '%s' "$path" | LC_ALL=C tr -cd 'A-Za-z0-9._/@+ -')
  [[ "$sanitized" == "$path" ]] || return 1
  IFS='/' read -r -a components <<< "$path"
  for component in "${components[@]}"; do
    [[ "$component" != "." && "$component" != ".." && "$component" != -* ]] || return 1
    component_fold=$(printf '%s' "$component" | LC_ALL=C tr '[:upper:]' '[:lower:]')
    component_stem=${component_fold%%.*}
    [[ "$component_fold" != ".git" && "$component" != *. && "$component" != *" " ]] ||
      return 1
    case "$component_stem" in
      aux | con | nul | prn | com[1-9] | lpt[1-9]) return 1 ;;
    esac
  done
}

path_traverses_symlink() {
  local path=$1 component current=""
  local -a components
  IFS='/' read -r -a components <<< "$path"
  for component in "${components[@]}"; do
    current=${current:+${current}/}${component}
    [[ ! -L "$current" ]] || return 0
  done
  return 1
}

path_key_value() {
  printf '%s' "$1" | LC_ALL=C tr '[:upper:]' '[:lower:]'
}

paths_overlap() {
  local left right
  left=$(path_key_value "$1")
  right=$(path_key_value "$2")
  [[ "$left" == "$right" || "$left" == "$right/"* || "$right" == "$left/"* ]]
}

managed_path_is_reserved() {
  local first_component first_fold path=$1 reserved
  first_component=${path%%/*}
  first_fold=$(path_key_value "$first_component")
  [[ "$first_fold" == .nautilus-engineering.tmp.* ]] && return 0
  for reserved in "$LOCK_FILE" "$MARKER_FILE" "$PROCESS_LOCK_FILE"; do
    paths_overlap "$path" "$reserved" && return 0
  done
  return 1
}

if ! validate_path "$LOCK_FILE"; then
  echo "Lock path must be a normalized repository-relative path: $LOCK_FILE" >&2
  exit 2
fi

REPO_ROOT=$(git rev-parse --show-toplevel)
cd "$REPO_ROOT"

if [[ -e "$PROCESS_LOCK_FILE" || -L "$PROCESS_LOCK_FILE" ]]; then
  echo "A Nautilus engineering sync is active or left an incomplete process lock" >&2
  exit 1
fi

TEMP_LOCK=""
cleanup() {
  if [[ -n "$TEMP_LOCK" ]]; then
    rm -f "$TEMP_LOCK"
  fi
}
trap cleanup EXIT

if [[ "$STAGED" == true ]]; then
  TEMP_LOCK=$(mktemp "${TMPDIR:-/tmp}/nautilus-engineering-lock.XXXXXX")
  if ! git show ":${LOCK_FILE}" > "$TEMP_LOCK" 2> /dev/null; then
    echo "Staged sync lock not found: $LOCK_FILE" >&2
    exit 1
  fi
  LOCK_SOURCE=$TEMP_LOCK
else
  if path_traverses_symlink "$LOCK_FILE" || [[ ! -f "$LOCK_FILE" ]]; then
    echo "Sync lock not found or not a regular file: $LOCK_FILE" >&2
    exit 1
  fi
  LOCK_SOURCE=$LOCK_FILE
fi

top_value() {
  local key=$1
  awk -F ' = ' -v key="$key" '
    $0 == "[[file]]" { in_file=1 }
    !in_file && $1 == key { value=$2; count++ }
    END {
      if (count != 1) exit 3
      print value
    }
  ' "$LOCK_SOURCE"
}

validate_lock_shape() {
  awk '
    NR == 1 && $0 != "version = 1" { exit 3 }
    NR == 2 && $0 !~ /^repository = "https:\/\/github\.com\/[A-Za-z0-9._-]+\/[A-Za-z0-9._-]+"$/ { exit 3 }
    NR == 3 && $0 !~ /^revision = "([0-9a-f]{40}|[0-9a-f]{64})"$/ { exit 3 }
    NR == 4 && $0 !~ /^manifest_sha256 = "[0-9a-f]{64}"$/ { exit 3 }
    NR == 5 && $0 !~ /^marker_file = "[A-Za-z0-9._\/@+ -]+"$/ { exit 3 }
    NR == 6 && $0 !~ /^profiles = \[("[A-Za-z0-9-]+"(, "[A-Za-z0-9-]+")*)?\]$/ { exit 3 }
    NR == 7 && $0 != "" { exit 3 }
    NR <= 7 { next }
    state == 0 {
      if ($0 != "[[file]]") exit 3
      state=1
      count++
      next
    }
    state == 1 {
      if ($0 !~ /^artifact = "[A-Za-z0-9-]+"$/) exit 3
      state=2
      next
    }
    state == 2 {
      if ($0 !~ /^path = "[A-Za-z0-9._\/@+ -]+"$/) exit 3
      state=3
      next
    }
    state == 3 {
      if ($0 !~ /^sha256 = "[0-9a-f]{64}"$/) exit 3
      state=4
      next
    }
    state == 4 {
      if ($0 !~ /^executable = (true|false)$/) exit 3
      state=5
      next
    }
    state == 5 {
      if ($0 != "") exit 3
      state=0
      next
    }
    END {
      if (NR < 12 || count == 0 || (state != 0 && state != 5)) exit 3
    }
  ' "$LOCK_SOURCE"
}

if ! validate_lock_shape; then
  echo "Sync lock has an invalid structure" >&2
  exit 2
fi

if ! version=$(top_value version) || [[ "$version" != "1" ]]; then
  echo "Sync lock has an invalid version" >&2
  exit 2
fi

REPOSITORY_PATTERN='^"https://github\.com/[A-Za-z0-9._/-]+"$'
REVISION_PATTERN='^"([0-9a-f]{40}|[0-9a-f]{64})"$'
HASH_PATTERN='^"[0-9a-f]{64}"$'
if ! repository=$(top_value repository) || ! [[ "$repository" =~ $REPOSITORY_PATTERN ]]; then
  echo "Sync lock has an invalid repository" >&2
  exit 2
fi
if ! revision=$(top_value revision) || ! [[ "$revision" =~ $REVISION_PATTERN ]]; then
  echo "Sync lock has an invalid revision" >&2
  exit 2
fi
if ! manifest_hash=$(top_value manifest_sha256) ||
  ! [[ "$manifest_hash" =~ $HASH_PATTERN ]]; then
  echo "Sync lock has an invalid manifest hash" >&2
  exit 2
fi
if ! MARKER_FILE=$(top_value marker_file); then
  echo "Sync lock has an invalid marker path" >&2
  exit 2
fi
MARKER_FILE=${MARKER_FILE#\"}
MARKER_FILE=${MARKER_FILE%\"}
if ! validate_path "$MARKER_FILE"; then
  echo "Sync lock has an invalid marker path" >&2
  exit 2
fi
if paths_overlap "$LOCK_FILE" "$MARKER_FILE" ||
  paths_overlap "$LOCK_FILE" "$PROCESS_LOCK_FILE" ||
  paths_overlap "$MARKER_FILE" "$PROCESS_LOCK_FILE"; then
  echo "Sync lock uses overlapping transaction paths" >&2
  exit 2
fi
if path_traverses_symlink "$MARKER_FILE" || [[ -e "$MARKER_FILE" ]]; then
  echo "An incomplete Nautilus engineering sync is marked in $REPO_ROOT" >&2
  exit 1
fi

lock_entries() {
  awk '
    function emit() {
      if (!in_file) return
      if (artifact == "" || path == "" || hash == "" || executable == "") exit 3
      print artifact "|" path "|" hash "|" executable
    }
    $0 == "[[file]]" {
      emit()
      in_file=1
      artifact=""
      path=""
      hash=""
      executable=""
      next
    }
    in_file && /^artifact = "/ {
      if (artifact != "") exit 3
      artifact=$0
      sub(/^artifact = "/, "", artifact)
      sub(/"$/, "", artifact)
      next
    }
    in_file && /^path = "/ {
      if (path != "") exit 3
      path=$0
      sub(/^path = "/, "", path)
      sub(/"$/, "", path)
      next
    }
    in_file && /^sha256 = "/ {
      if (hash != "") exit 3
      hash=$0
      sub(/^sha256 = "/, "", hash)
      sub(/"$/, "", hash)
      next
    }
    in_file && /^executable = / {
      if (executable != "") exit 3
      executable=$3
      next
    }
    END { emit() }
  ' "$LOCK_SOURCE"
}

hash_file() {
  if [[ "$HASH_COMMAND" == sha256sum ]]; then
    sha256sum "$1" | awk '{ print $1 }'
  else
    shasum -a 256 "$1" | awk '{ print $1 }'
  fi
}

hash_staged() {
  if [[ "$HASH_COMMAND" == sha256sum ]]; then
    git show ":$1" | sha256sum | awk '{ print $1 }'
  else
    git show ":$1" | shasum -a 256 | awk '{ print $1 }'
  fi
}

failures=0
count=0
SEEN_PATH_KEYS=()
SEEN_ARTIFACTS=()
entries_status=0
entries=$(lock_entries) || entries_status=$?
if ((entries_status != 0)); then
  echo "Sync lock contains an incomplete file entry" >&2
  exit 2
fi

while IFS='|' read -r artifact path expected_hash executable; do
  [[ -n "$artifact" ]] || continue
  count=$((count + 1))
  if ! [[ "$artifact" =~ ^[A-Za-z0-9-]+$ ]] || ! validate_path "$path" ||
    managed_path_is_reserved "$path" ||
    ! [[ "$expected_hash" =~ ^[0-9a-f]{64}$ ]] ||
    [[ "$executable" != true && "$executable" != false ]]; then
    echo "Invalid sync lock entry: $path" >&2
    exit 2
  fi
  path_key=$(path_key_value "$path")
  for seen_key in "${SEEN_PATH_KEYS[@]:-}"; do
    if [[ "$seen_key" == "$path_key" || "$seen_key" == "$path_key/"* ||
      "$path_key" == "$seen_key/"* ]]; then
      echo "Overlapping or case-colliding sync lock path: $path" >&2
      exit 2
    fi
  done
  for seen_artifact in "${SEEN_ARTIFACTS[@]:-}"; do
    if [[ "$seen_artifact" == "$artifact" ]]; then
      echo "Duplicate sync lock artifact: $artifact" >&2
      exit 2
    fi
  done
  SEEN_PATH_KEYS+=("$path_key")
  SEEN_ARTIFACTS+=("$artifact")

  entry_failed=false

  if [[ "$STAGED" == true ]]; then
    stage_lines=$(git ls-files --stage -- "$path")
    if [[ -z "$stage_lines" || "$(printf '%s\n' "$stage_lines" | awk 'END { print NR }')" != "1" ]]; then
      echo "FAIL $path: staged file is missing or unmerged"
      failures=$((failures + 1))
      continue
    fi
    actual_mode=$(printf '%s\n' "$stage_lines" | awk '{ print $1 }')
    expected_mode=100644
    if [[ "$executable" == true ]]; then
      expected_mode=100755
    fi
    if [[ "$actual_mode" != "$expected_mode" ]]; then
      echo "FAIL $path: staged mode is $actual_mode, expected $expected_mode"
      failures=$((failures + 1))
      entry_failed=true
    fi
    if ! actual_hash=$(hash_staged "$path" 2> /dev/null); then
      echo "FAIL $path: staged content is unavailable"
      failures=$((failures + 1))
      continue
    fi
  else
    if path_traverses_symlink "$path" || [[ ! -f "$path" ]]; then
      echo "FAIL $path: file is missing, a symlink, or not regular"
      failures=$((failures + 1))
      continue
    fi
    if [[ "$CHECK_WORKTREE_MODE" == true && "$executable" == true && ! -x "$path" ]]; then
      echo "FAIL $path: file is not executable"
      failures=$((failures + 1))
      entry_failed=true
    elif [[ "$CHECK_WORKTREE_MODE" == true && "$executable" == false && -x "$path" ]]; then
      echo "FAIL $path: file is unexpectedly executable"
      failures=$((failures + 1))
      entry_failed=true
    fi
    actual_hash=$(hash_file "$path")
  fi

  if [[ "$actual_hash" != "$expected_hash" ]]; then
    echo "FAIL $path: content differs from the sync lock"
    failures=$((failures + 1))
  elif [[ "$entry_failed" == false ]]; then
    echo "OK   $path"
  fi
done <<< "$entries"

if ((count == 0)); then
  echo "Sync lock contains no files" >&2
  exit 2
fi
if ((failures > 0)); then
  echo "$failures synced file check(s) failed" >&2
  exit 1
fi

if [[ "$STAGED" == false && "$CHECK_WORKTREE_MODE" == false ]]; then
  echo "All $count synced file contents match the lock; executable modes were not checked"
else
  echo "All $count synced files match the lock"
fi
