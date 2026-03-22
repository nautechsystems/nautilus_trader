#!/usr/bin/env bash
# Run cargo clippy only on crates with staged changes.
# Falls back to full workspace for clean checkouts, workspace-level config
# changes, or when no crate-level changes can be identified.
set -euo pipefail

DESIRED_FEATURES=(ffi python high-precision defi)
PROFILE="nextest"

run_full() {
  echo "Running full workspace clippy"
  exec cargo clippy --workspace --all-targets \
    --features "$(
      IFS=,
      echo "${DESIRED_FEATURES[*]}"
    )" \
    --profile "$PROFILE" -- -D warnings
}

# Get staged .rs and .toml files; fall back to unstaged diff
changed_files=$(git diff --cached --name-only --diff-filter=ACMR -- '*.rs' '*.toml' 2> /dev/null || true)
if [ -z "$changed_files" ]; then
  changed_files=$(git diff --name-only HEAD -- '*.rs' '*.toml' 2> /dev/null || true)
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

feat_args=()
if [ -n "$feat_seen" ]; then
  feat_str="${feat_seen## }"
  feat_str="${feat_str// /,}"
  feat_args=("--features" "$feat_str")
fi

echo "Running clippy on: ${seen_list[*]}"
cargo clippy "${pkg_args[@]}" --all-targets "${feat_args[@]}" \
  --profile "$PROFILE" -- -D warnings
