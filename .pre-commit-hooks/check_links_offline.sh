#!/usr/bin/env bash
# Checks that internal links in markdown and Jupytext docs resolve on disk.
#
# Runs lychee in offline mode to catch broken relative paths, anchors, and
# cross-references without hitting the network. External URLs are ignored
# here; they're covered by the periodic `make docs-check-links` audit.
#
# If lychee is not installed, the hook exits 0 with a warning so that
# contributors without the cargo toolchain can still commit.

set -euo pipefail

YELLOW='\033[0;33m'
NC='\033[0m'

if ! command -v lychee > /dev/null 2>&1; then
  echo -e "${YELLOW}Warning:${NC} lychee not found, skipping offline link check" >&2
  echo "  Install with: cargo install lychee --locked" >&2
  exit 0
fi

# pre-commit passes changed files as arguments. If none are passed (no
# relevant files in the commit), skip; use `make docs-check-links` for a
# full audit.
if [ $# -eq 0 ]; then
  exit 0
fi

repo_root=$(git rev-parse --show-toplevel)

lychee \
  --no-progress \
  --offline \
  --include-fragments \
  --root-dir "$repo_root" \
  --fallback-extensions md,py,html \
  --exclude-path .venv \
  --exclude-path target \
  --exclude-path docs/python-api-latest \
  --exclude "file://.*/python-api-latest/.*" \
  "$@"
