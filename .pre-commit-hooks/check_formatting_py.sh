#!/usr/bin/env bash

set -euo pipefail

REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
exec env VIRTUAL_ENV= uv run --project "$REPO_ROOT/python" --no-sync \
  python -B "$REPO_ROOT/scripts/format-python.py" "$@"
