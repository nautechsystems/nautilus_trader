#!/usr/bin/env bash
set -euo pipefail

# Single source of truth for the uv project environment. The Python project lives in `python/`, so
# uv would otherwise default to `python/.venv`, while the repository-root `.venv` is the documented
# environment and the one Make, the editors, and PYO3_PYTHON use. Honours a caller's own value.

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
printf '%s\n' "${UV_PROJECT_ENVIRONMENT:-$repo_root/.venv}"
