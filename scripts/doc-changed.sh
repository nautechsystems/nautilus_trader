#!/usr/bin/env bash
# Run cargo doc only on crates with staged changes.
# Falls back to full workspace for clean checkouts, workspace-level config
# changes, or when no crate-level changes can be identified.
set -euo pipefail

DESIRED_FEATURES=(ffi python high-precision defi)
PROFILE="${CARGO_CI_PROFILE:-nextest}"
export HIGH_PRECISION="${HIGH_PRECISION:-1}"

run_full() {
  echo "Running full workspace doc check"
  exec cargo doc --workspace --no-deps --quiet \
    --features "$(
      IFS=,
      echo "${DESIRED_FEATURES[*]}"
    )" \
    --profile "$PROFILE"
}

# Get staged .rs and .toml files; fall back to unstaged diff
changed_files=$(git diff --cached --name-only --diff-filter=ACMR -- '*.rs' '*.toml' 2> /dev/null || true)
if [ -z "$changed_files" ]; then
  changed_files=$(git diff --name-only HEAD -- '*.rs' '*.toml' 2> /dev/null || true)
fi

# CI fallback: clean checkouts have no diff vs HEAD; derive changed files
# from CHANGED_BASE_SHA (exported by the workflow as the PR base or push before SHA).
if [ -z "$changed_files" ] &&
  [ -n "${CHANGED_BASE_SHA:-}" ] &&
  [ "$CHANGED_BASE_SHA" != "0000000000000000000000000000000000000000" ]; then
  base=$(git merge-base "$CHANGED_BASE_SHA" HEAD 2> /dev/null || true)
  if [ -n "$base" ]; then
    changed_files=$(git diff --name-only "$base"..HEAD -- '*.rs' '*.toml' 2> /dev/null || true)
  fi
fi

# Clean checkout (CI --all-files) or no Rust/TOML changes at all
if [ -z "$changed_files" ]; then
  run_full
fi

# Workspace-level files that affect all crates
if echo "$changed_files" | grep -qE '^(Cargo\.toml|clippy\.toml|rust-toolchain\.toml|\.cargo/)'; then
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

# Unrecognized paths (non-crate TOML files matched by pre-commit filter)
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

  for feat in "${DESIRED_FEATURES[@]}"; do
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

echo "Running doc check on: ${seen_list[*]}"
# `${feat_args[@]+...}` guards the expansion: bash 3.2 (macOS default) treats an
# empty array as unbound under `set -u`, which fires when no features are needed.
cargo doc "${pkg_args[@]}" --no-deps --quiet ${feat_args[@]+"${feat_args[@]}"} \
  --profile "$PROFILE"
