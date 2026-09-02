#!/usr/bin/env bash
# Run cargo clippy only on crates with staged Rust build input changes.
# Falls back to full workspace for clean checkouts, workspace-level Rust config
# changes, or when no crate-level changes can be identified.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# One shared definition so this pass resolves the same feature graph as the Makefile
# gates and the other changed-crate hook. A command substitution inside a here-string
# does not trip errexit, so bind the list first and reject an empty one rather than
# silently running cargo with no features.
FEATURE_LIST="$(bash "$SCRIPT_DIR/cargo-features.bash")"
[[ -n "$FEATURE_LIST" ]] || {
  echo "Error: cargo-features.bash produced no features" >&2
  exit 1
}
IFS=, read -ra DESIRED_FEATURES <<< "$FEATURE_LIST"
PROFILE="${CARGO_CI_PROFILE:-nextest}"
export HIGH_PRECISION="${HIGH_PRECISION:-1}"
resolved_changed_base=0

select_rust_inputs() {
  while IFS= read -r file; do
    case "$file" in
      *.rs | Cargo.toml | */Cargo.toml | Cargo.lock | clippy.toml | rust-toolchain.toml | python/pyproject.toml | .cargo/config.toml | scripts/cargo-features.bash)
        printf '%s\n' "$file"
        ;;
    esac
  done
}

run_full() {
  echo "Running full workspace clippy"
  exec cargo clippy --workspace --lib --bins --tests \
    --features "$(
      IFS=,
      echo "${DESIRED_FEATURES[*]}"
    )" \
    --profile "$PROFILE" -- -D warnings
}

# Get staged candidate files; fall back to unstaged diff
changed_files=$(git diff --cached --name-only --diff-filter=ACMR -- '*.rs' '*.toml' 'Cargo.lock' 'scripts/cargo-features.bash' 2> /dev/null || true)
if [ -z "$changed_files" ]; then
  changed_files=$(git diff --name-only HEAD -- '*.rs' '*.toml' 'Cargo.lock' 'scripts/cargo-features.bash' 2> /dev/null || true)
fi

# CI fallback: clean checkouts have no diff vs HEAD; derive changed files
# from CHANGED_BASE_SHA (exported by the workflow as the PR base or push before SHA).
if [ -z "$changed_files" ] &&
  [ -n "${CHANGED_BASE_SHA:-}" ] &&
  [ "$CHANGED_BASE_SHA" != "0000000000000000000000000000000000000000" ]; then
  base=$(git merge-base "$CHANGED_BASE_SHA" HEAD 2> /dev/null || true)
  if [ -n "$base" ]; then
    resolved_changed_base=1
    changed_files=$(git diff --name-only "$base"..HEAD -- '*.rs' '*.toml' 'Cargo.lock' 'scripts/cargo-features.bash' 2> /dev/null || true)
  fi
fi

if [ -z "$changed_files" ]; then
  if [ "$resolved_changed_base" -eq 1 ]; then
    echo "No Rust/TOML changes detected; skipping clippy"
    exit 0
  fi

  # Clean checkout or unresolved changed-file state
  run_full
fi

changed_files=$(printf '%s\n' "$changed_files" | select_rust_inputs)
if [ -z "$changed_files" ]; then
  echo "No Rust build inputs detected; skipping clippy"
  exit 0
fi

# Workspace-level files that affect all crates
if echo "$changed_files" | grep -qE '^(Cargo\.toml|Cargo\.lock|clippy\.toml|rust-toolchain\.toml|\.cargo/config\.toml|scripts/cargo-features\.bash)'; then
  run_full
fi

# Collect unique crate packages from changed file paths
seen=""
seen_list=()

for file in $changed_files; do
  if [[ "$file" =~ ^crates/adapters/([^/]+)/ ]]; then
    pkg="nautilus-${BASH_REMATCH[1]}"
    pkg="${pkg//_/-}"
  elif [[ "$file" =~ ^crates/persistence/macros/ ]]; then
    pkg="nautilus-persistence-macros"
  elif [[ "$file" =~ ^crates/([^/]+)/ ]]; then
    name="${BASH_REMATCH[1]}"
    [[ "$name" == "adapters" ]] && continue
    pkg="nautilus-${name}"
    pkg="${pkg//_/-}"
  elif [[ "$file" =~ ^crates/Cargo\.toml$ ]] || [[ "$file" =~ ^crates/lib\.rs$ ]]; then
    pkg="nautilus-trader"
  else
    continue
  fi

  case " $seen " in
    *" $pkg "*) ;;
    *)
      seen="$seen $pkg"
      seen_list+=("$pkg")
      ;;
  esac
done

# Unrecognized Rust input paths
if [ ${#seen_list[@]} -eq 0 ]; then
  run_full
fi

# Build package args and resolve applicable features per package
pkg_args=()
feat_seen=""

for pkg in "${seen_list[@]}"; do
  pkg_args+=("-p" "$pkg")

  pkg_features=$(cargo metadata --format-version 1 --no-deps 2> /dev/null |
    python3 -c "
import json, sys
data = json.load(sys.stdin)
for p in data['packages']:
    if p['name'] == '$pkg':
        print(' '.join(p['features'].keys()))
        break
" 2> /dev/null || true)

  desired_features="${DESIRED_FEATURES[*]}"
  if [ "$pkg" = "nautilus-serialization" ]; then
    # The crate has no default features, so compile each core format when its source changes
    desired_features="$desired_features arrow capnp display sbe"
  fi

  for feat in $desired_features; do
    case " $pkg_features " in
      *" $feat "*)
        case " $feat_seen " in
          *" $feat "*) ;;
          *) feat_seen="$feat_seen $feat" ;;
        esac
        ;;
    esac
  done
done

# When 'defi' is enabled on nautilus-common, Cargo feature unification adds the
# DeFi variant to DataEvent for all consumers. nautilus-live matches on DataEvent
# and gates its arm behind its own 'defi' feature, so it must be in the package
# list to receive the feature flag and compile the match arm.
if [[ " $feat_seen " == *" defi "* ]]; then
  case " $seen " in
    *" nautilus-live "*) ;;
    *)
      seen="$seen nautilus-live"
      seen_list+=("nautilus-live")
      pkg_args+=("-p" "nautilus-live")
      ;;
  esac
fi

feat_args=()
if [ -n "$feat_seen" ]; then
  feat_str="${feat_seen## }"
  feat_str="${feat_str// /,}"
  feat_args=("--features" "$feat_str")
fi

echo "Running clippy on: ${seen_list[*]}"
# `${feat_args[@]+...}` guards the expansion: bash 3.2 (macOS default) treats an
# empty array as unbound under `set -u`, which fires when no features are needed.
cargo clippy "${pkg_args[@]}" --lib --bins --tests ${feat_args[@]+"${feat_args[@]}"} \
  --profile "$PROFILE" -- -D warnings
