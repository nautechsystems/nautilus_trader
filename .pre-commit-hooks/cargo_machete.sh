#!/usr/bin/env bash
# Runs cargo-machete on changed Rust packages to detect declared-but-unused dependencies.
# Falls back to all maintained packages for clean checkouts, unresolved CI bases,
# or changes to this script.
#
# False positives are managed via [package.metadata.cargo-machete] ignored
# lists in each crate's Cargo.toml. Known categories:
#   - Feature-graph plumbing (optional deps referenced only in [features])
#   - Macro-expansion-only deps (brought into scope by a derive elsewhere)
#   - Build-script deps (cargo-machete cannot always see feature-gated build.rs uses)

set -euo pipefail

PINNED_VERSION="0.9.2"
resolved_changed_base=0

print_failure_guidance() {
  echo ""
  echo "If a flagged dependency is a false positive (feature-gate plumbing,"
  echo "macro expansion, etc.), add it to the crate's Cargo.toml with a"
  echo "comment explaining why machete cannot see the use:"
  echo ""
  echo "    [package.metadata.cargo-machete]"
  echo "    ignored = [\"crate-name\"] # why machete is wrong"
  echo ""
}

run_all_packages() {
  # The standalone Lighter quickstart has branch-based git dependencies and no
  # committed lockfile. Scan it without metadata so this hook stays offline.
  echo "Running cargo machete on all maintained Rust packages..."
  cargo machete --with-metadata \
    Cargo.toml \
    crates \
    examples/tutorials || return 1
  cargo machete examples/quickstarts/lighter-rust-data-client || return 1
}

find_manifest() {
  local file="$1"
  local dir

  case "$file" in
    Cargo.toml | */Cargo.toml)
      [ -f "$file" ] || return 1
      printf '%s\n' "$file"
      return
      ;;
  esac

  dir=${file%/*}
  [ "$dir" != "$file" ] || dir="."

  while :; do
    if [ -f "$dir/Cargo.toml" ]; then
      if [ "$dir" = "." ]; then
        printf '%s\n' "Cargo.toml"
      else
        printf '%s\n' "$dir/Cargo.toml"
      fi
      return
    fi

    [ "$dir" != "." ] || return 1
    case "$dir" in
      */*) dir=${dir%/*} ;;
      *) dir="." ;;
    esac
  done
}

# Prefer the staged candidate, then an unstaged working-tree change. CI has a
# clean checkout, so resolve its configured base only when neither local diff exists.
changed_files=$(git diff --cached --name-only --no-renames 2> /dev/null || true)
if [ -z "$changed_files" ]; then
  changed_files=$(git diff --name-only --no-renames HEAD 2> /dev/null || true)
fi

if [ -z "$changed_files" ] &&
  [ -n "${CHANGED_BASE_SHA:-}" ] &&
  [ "$CHANGED_BASE_SHA" != "0000000000000000000000000000000000000000" ]; then
  base=$(git merge-base "$CHANGED_BASE_SHA" HEAD 2> /dev/null || true)
  if [ -n "$base" ]; then
    resolved_changed_base=1
    changed_files=$(git diff --name-only --no-renames "$base"..HEAD 2> /dev/null || true)
  fi
fi

full_scan=0
workspace_manifests=()
workspace_seen=""
quickstart_changed=0

if [ -z "$changed_files" ]; then
  if [ "$resolved_changed_base" -eq 1 ]; then
    echo "No changes detected; skipping cargo machete"
    exit 0
  fi
  full_scan=1
else
  while IFS= read -r file; do
    case "$file" in
      .pre-commit-hooks/cargo_machete.sh)
        full_scan=1
        break
        ;;
      Cargo.toml)
        full_scan=1
        break
        ;;
      crates/* | examples/tutorials/* | examples/quickstarts/lighter-rust-data-client/*)
        case "$file" in
          *.rs | */Cargo.toml) ;;
          *) continue ;;
        esac
        ;;
      *) continue ;;
    esac

    manifest=$(find_manifest "$file" || true)
    if [ -z "$manifest" ]; then
      full_scan=1
      break
    fi

    case "$manifest" in
      examples/quickstarts/lighter-rust-data-client/Cargo.toml)
        quickstart_changed=1
        ;;
      crates/Cargo.toml | crates/*/Cargo.toml | examples/tutorials/Cargo.toml)
        case " $workspace_seen " in
          *" $manifest "*) ;;
          *)
            workspace_seen="$workspace_seen $manifest"
            workspace_manifests+=("$manifest")
            ;;
        esac
        ;;
      *)
        full_scan=1
        break
        ;;
    esac
  done <<< "$changed_files"
fi

if [ "$full_scan" -eq 0 ] && [ ${#workspace_manifests[@]} -eq 0 ] && [ "$quickstart_changed" -eq 0 ]; then
  echo "No maintained Rust package changes detected; skipping cargo machete"
  exit 0
fi

if ! command -v cargo-machete &> /dev/null; then
  echo "ERROR: cargo-machete ${PINNED_VERSION} is required for the unused-dependency check" >&2
  echo "       install with: cargo install --locked cargo-machete@${PINNED_VERSION}" >&2
  exit 1
fi

installed_version=$(cargo machete --version 2> /dev/null | tr -d '[:space:]')
if [ "$installed_version" != "$PINNED_VERSION" ]; then
  echo "WARNING: cargo-machete ${installed_version} differs from pinned ${PINNED_VERSION}"
  echo "         detection heuristics may drift; consider:"
  echo "         cargo install --locked cargo-machete@${PINNED_VERSION}"
fi

if [ "$full_scan" -eq 1 ]; then
  if ! run_all_packages; then
    print_failure_guidance
    exit 1
  fi
  exit 0
fi

if [ ${#workspace_manifests[@]} -gt 0 ]; then
  echo "Running cargo machete on: ${workspace_manifests[*]}"
  if ! cargo machete --with-metadata "${workspace_manifests[@]}"; then
    print_failure_guidance
    exit 1
  fi
fi

if [ "$quickstart_changed" -eq 1 ]; then
  echo "Running cargo machete on: examples/quickstarts/lighter-rust-data-client/Cargo.toml"
  if ! cargo machete examples/quickstarts/lighter-rust-data-client/Cargo.toml; then
    print_failure_guidance
    exit 1
  fi
fi
