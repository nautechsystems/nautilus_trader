#!/usr/bin/env bash
# Update Cargo dependencies, then enforce the release cooldown transactionally

set -euo pipefail

main() {
  if (($# != 0)); then
    echo "Usage: scripts/update-cargo-dependencies.bash" >&2
    return 2
  fi

  local script_dir repo_root lock status
  script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  repo_root="$(cd "${script_dir}/.." && pwd)"
  LOCKS=(
    Cargo.lock
    crates/adapters/lighter/fuzz/pornin/Cargo.lock
  )

  cd "$repo_root"

  snapshot_dir=$(mktemp -d "${TMPDIR:-/tmp}/nautilus-cargo-update.XXXXXX")
  restore_required=false
  trap cleanup EXIT

  for lock in "${LOCKS[@]}"; do
    if [[ ! -f "$lock" ]]; then
      echo "Cargo lockfile not found: $lock" >&2
      return 1
    fi
    mkdir -p "${snapshot_dir}/$(dirname "$lock")"
    cp -p "$lock" "${snapshot_dir}/${lock}"
  done

  restore_required=true
  if cargo update; then
    :
  else
    status=$?
    echo "Cargo dependency update failed." >&2
    return "$status"
  fi

  if bash scripts/check-cargo-cooldown.sh \
    --fix \
    --base HEAD \
    --snapshot-dir "$snapshot_dir" \
    --lock Cargo.lock \
    --lock crates/adapters/lighter/fuzz/pornin/Cargo.lock; then
    :
  else
    status=$?
    echo "Cargo cooldown repair failed." >&2
    return "$status"
  fi

  restore_required=false
  echo "Cargo dependency update complete; cooldown policy enforced"
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
    fi
    if ((status == 0)); then
      status=1
    fi
  fi
  if ((restore_status == 0)); then
    rm -rf "$snapshot_dir"
  fi
  exit "$status"
}

main "$@"
