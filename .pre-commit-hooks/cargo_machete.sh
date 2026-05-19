#!/usr/bin/env bash
# Runs cargo-machete to detect declared-but-unused dependencies.
#
# False positives are managed via [package.metadata.cargo-machete] ignored
# lists in each crate's Cargo.toml. Known categories:
#   - Feature-graph plumbing (optional deps referenced only in [features])
#   - Macro-expansion-only deps (brought into scope by a derive elsewhere)
#   - Build-script deps (handled by default invocation; do not use --with-metadata)

set -euo pipefail

PINNED_VERSION="0.9.2"

if ! command -v cargo-machete &> /dev/null; then
  echo "INFO: cargo-machete not installed, skipping unused-dependency check"
  echo "      install with: cargo install --locked cargo-machete@${PINNED_VERSION}"
  exit 0
fi

installed_version=$(cargo machete --version 2> /dev/null | tr -d '[:space:]')
if [ "$installed_version" != "$PINNED_VERSION" ]; then
  echo "WARNING: cargo-machete ${installed_version} differs from pinned ${PINNED_VERSION}"
  echo "         detection heuristics may drift; consider:"
  echo "         cargo install --locked cargo-machete@${PINNED_VERSION}"
fi

echo "Running cargo machete..."
if ! cargo machete; then
  echo ""
  echo "If a flagged dependency is a false positive (feature-gate plumbing,"
  echo "macro expansion, etc.), add it to the crate's Cargo.toml with a"
  echo "comment explaining why machete cannot see the use:"
  echo ""
  echo "    [package.metadata.cargo-machete]"
  echo "    ignored = [\"crate-name\"] # why machete is wrong"
  echo ""
  exit 1
fi
