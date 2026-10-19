#!/usr/bin/env bash
# Locks the resolved Jiff feature policy for Rust-only and Python-extension builds.

set -euo pipefail

REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$REPO_ROOT"

resolved_features() {
  cargo tree --locked -p "$1" -e features -i jiff --prefix none |
    sed -n 's/^jiff feature "\([^"]*\)".*/\1/p' |
    sort -u
}

check_feature_set() {
  local package="$1"
  shift

  local actual expected
  actual=$(resolved_features "$package")
  expected=$(printf '%s\n' "$@" | sort -u)

  if [[ "$actual" == "$expected" ]]; then
    return
  fi

  echo "Jiff feature policy changed for $package" >&2
  echo "Expected:" >&2
  printf '%s\n' "$expected" >&2
  echo "Actual:" >&2
  printf '%s\n' "$actual" >&2
  exit 1
}

check_workspace_dependency_declarations() {
  local allowed_declaration
  allowed_declaration='^[[:space:]]*jiff[[:space:]]*=[[:space:]]*\{[[:space:]]*workspace[[:space:]]*=[[:space:]]*true([[:space:]]*,[[:space:]]*optional[[:space:]]*=[[:space:]]*true)?[[:space:]]*\}[[:space:]]*$'

  local violations=()
  while IFS=: read -r manifest line declaration; do
    [[ -z "$manifest" ]] && continue
    if [[ ! "$declaration" =~ $allowed_declaration ]]; then
      violations+=("$manifest:$line:$declaration")
    fi
  done < <(
    rg -n --no-heading '^[[:space:]]*jiff[[:space:]]*=' crates --glob Cargo.toml 2> /dev/null || true
  )

  while IFS= read -r violation; do
    [[ -z "$violation" ]] && continue
    violations+=("$violation")
  done < <(
    rg -n --no-heading \
      "^\\[[^]]*dependencies\\.(['\"]?jiff['\"]?)\\]|package[[:space:]]*=[[:space:]]*['\"]jiff['\"]|^[[:space:]]*['\"]jiff['\"][[:space:]]*=|jiff(?:\\?)?/" \
      crates --glob Cargo.toml 2> /dev/null || true
  )

  if ((${#violations[@]} == 0)); then
    return
  fi

  echo "Workspace crates must inherit Jiff without adding features" >&2
  printf '  %s\n' "${violations[@]}" >&2
  exit 1
}

check_patch_dependency_declarations() {
  if ! command -v jq > /dev/null; then
    echo "jq is required to validate Jiff dependencies in maintained patches" >&2
    exit 1
  fi

  local violations
  violations=$(
    cargo metadata --locked --format-version 1 |
      jq -r '
        .packages[]
        | select((.manifest_path | gsub("\\\\"; "/")) | contains("/patches/"))
        | . as $package
        | .dependencies[]
        | select(.name == "jiff")
        | select(
            .rename != null
            or .uses_default_features
            or any(
              .features[];
              . == "default" or . == "tz-system" or startswith("tzdb-")
            )
          )
        | "\($package.manifest_path): Jiff dependency must disable defaults and omit tzdb features"
      '
  )

  local forwarded_features
  forwarded_features=$(
    rg -n --no-heading 'jiff(?:\?)?/' patches --glob Cargo.toml 2> /dev/null || true
  )

  if [[ -z "$violations" && -z "$forwarded_features" ]]; then
    return
  fi

  echo "Maintained patches must keep Jiff independent of system time zone sources" >&2
  [[ -n "$violations" ]] && printf '  %s\n' "$violations" >&2
  [[ -n "$forwarded_features" ]] && printf '  %s\n' "$forwarded_features" >&2
  exit 1
}

# Workspace crates inherit the root feature policy. This prevents an adapter-only build from
# enabling a system time zone source that the two representative feature graphs would not expose.
check_workspace_dependency_declarations

# Maintained patches cannot inherit workspace dependencies, so validate their resolved dependency
# declarations separately while still forbidding system and bundled time zone database features.
check_patch_dependency_declarations

# Rust-only paths intentionally use the bundled database with no system source.
check_feature_set nautilus-trading \
  alloc perf-inline serde std tz-fat tzdb-bundle-always

# PyO3 currently enables Jiff defaults transitively. Nautilus lookups still use the bundled
# database explicitly; keeping this set locked makes any upstream feature-policy change visible.
check_feature_set nautilus-pyo3 \
  alloc default perf-inline serde std tz-fat tz-system tzdb-bundle-always \
  tzdb-bundle-platform tzdb-concatenated tzdb-zoneinfo

echo "Jiff feature policy checks passed"
