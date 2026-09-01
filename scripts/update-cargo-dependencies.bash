#!/usr/bin/env bash
# Update Cargo dependencies, then enforce the release cooldown transactionally

set -euo pipefail

main() {
  local script_dir repo_root lock manifest status seen
  local -a check_args
  script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  repo_root="$(cd "${script_dir}/.." && pwd)"
  LOCKS=()
  snapshot_dir=""
  restore_required=false
  transaction_lock=""
  retain_transaction_lock=false

  while (($# > 0)); do
    case "$1" in
      --lock)
        (($# >= 2)) || {
          echo "--lock requires a value" >&2
          return 2
        }
        LOCKS+=("$2")
        shift 2
        ;;
      -h | --help)
        echo "Usage: scripts/update-cargo-dependencies.bash [--lock PATH]..."
        return 0
        ;;
      *)
        echo "Unknown argument: $1" >&2
        return 2
        ;;
    esac
  done

  cd "$repo_root"

  if ((${#LOCKS[@]} == 0)); then
    while IFS= read -r -d '' tracked_path; do
      if [[ "$tracked_path" == "Cargo.lock" || "$tracked_path" == */Cargo.lock ]]; then
        LOCKS+=("$tracked_path")
      fi
    done < <(git ls-files -z)
  fi
  if ((${#LOCKS[@]} == 0)); then
    echo "No tracked Cargo.lock files found; pass --lock for an untracked lockfile." >&2
    return 2
  fi

  VALIDATED_LOCKS=()
  for lock in "${LOCKS[@]}"; do
    validate_lock_path "$lock" || return 2
    for seen in "${VALIDATED_LOCKS[@]:-}"; do
      if [[ "$seen" == "$lock" ]]; then
        echo "Cargo lock path was supplied more than once: $lock" >&2
        return 2
      fi
    done
    VALIDATED_LOCKS+=("$lock")
  done
  LOCKS=("${VALIDATED_LOCKS[@]}")

  transaction_lock=$(git rev-parse --git-path nautilus-cargo-update.lock)
  if ! mkdir "$transaction_lock" 2> /dev/null; then
    echo "Another Cargo dependency update is active or left a transaction lock: $transaction_lock" >&2
    return 2
  fi
  trap cleanup EXIT

  snapshot_dir=$(mktemp -d "${TMPDIR:-/tmp}/nautilus-cargo-update.XXXXXX")

  for lock in "${LOCKS[@]}"; do
    mkdir -p "${snapshot_dir}/$(dirname "$lock")"
    cp -p "$lock" "${snapshot_dir}/${lock}"
  done

  restore_required=true
  for lock in "${LOCKS[@]}"; do
    manifest=$(manifest_for_lock "$lock")
    if [[ "$manifest" == "Cargo.toml" ]]; then
      if cargo update; then
        continue
      else
        status=$?
      fi
    elif cargo update --manifest-path "$manifest"; then
      continue
    else
      status=$?
    fi
    echo "Cargo dependency update failed for $manifest." >&2
    return "$status"
  done

  check_args=(--fix --base HEAD --snapshot-dir "$snapshot_dir")
  for lock in "${LOCKS[@]}"; do
    check_args+=(--lock "$lock")
  done
  if bash scripts/check-cargo-cooldown.sh "${check_args[@]}"; then
    :
  else
    status=$?
    echo "Cargo cooldown repair failed." >&2
    return "$status"
  fi

  restore_required=false
  echo "Cargo dependency update complete; cooldown policy enforced"
}

manifest_for_lock() {
  local lock_dir
  lock_dir=$(dirname "$1")
  if [[ "$lock_dir" == "." ]]; then
    printf 'Cargo.toml\n'
  else
    printf '%s/Cargo.toml\n' "$lock_dir"
  fi
}

validate_lock_path() {
  local lock=$1 component current="" manifest

  if [[ -z "$lock" || "$lock" == /* || "$lock" == *\\* || "$lock" == *$'\n'* ]]; then
    echo "Cargo lock path must be a repository-relative POSIX path: $lock" >&2
    return 1
  fi
  IFS='/' read -r -a components <<< "$lock"
  for component in "${components[@]}"; do
    if [[ -z "$component" || "$component" == "." || "$component" == ".." ]]; then
      echo "Cargo lock path contains an unsafe component: $lock" >&2
      return 1
    fi
    current=${current:+${current}/}${component}
    if [[ -L "$current" ]]; then
      echo "Cargo lock path must not traverse a symlink: $lock" >&2
      return 1
    fi
  done
  if [[ "${components[${#components[@]} - 1]}" != "Cargo.lock" || ! -f "$lock" ]]; then
    echo "Cargo lockfile not found or not named Cargo.lock: $lock" >&2
    return 1
  fi
  manifest=$(manifest_for_lock "$lock")
  if [[ ! -f "$manifest" ]]; then
    echo "Cargo manifest not found for $lock: $manifest" >&2
    return 1
  fi
}

restore_locks() {
  local lock restore_tmp status=0
  for lock in "${LOCKS[@]}"; do
    restore_tmp=""
    if ! restore_tmp=$(mktemp "$(dirname "$lock")/.${lock##*/}.restore.XXXXXX") ||
      ! cp -p "${snapshot_dir}/${lock}" "$restore_tmp" ||
      ! mv -f "$restore_tmp" "$lock"; then
      if [[ -n "$restore_tmp" ]]; then
        rm -f "$restore_tmp"
      fi
      echo "Could not restore $lock from its pre-update snapshot." >&2
      status=1
    fi
  done
  return "$status"
}

cleanup() {
  local status=$? restore_status=0
  trap - EXIT
  set +e
  if [[ "$restore_required" == true ]]; then
    restore_locks || restore_status=$?
    if ((restore_status == 0)); then
      echo "Cargo dependency update restored the pre-update lockfiles" >&2
    else
      echo "Cargo dependency update could not restore every pre-update lockfile." >&2
      echo "Pre-update snapshots retained at $snapshot_dir" >&2
      echo "Transaction lock retained at $transaction_lock" >&2
      retain_transaction_lock=true
    fi
    if ((status == 0)); then
      status=1
    fi
  fi
  if ((restore_status == 0)) && [[ -n "$snapshot_dir" ]]; then
    rm -rf "$snapshot_dir"
  fi
  if [[ "$retain_transaction_lock" == false && -n "$transaction_lock" ]] &&
    ! rmdir "$transaction_lock"; then
    echo "Could not remove Cargo dependency transaction lock: $transaction_lock" >&2
    status=1
  fi
  exit "$status"
}

main "$@"
