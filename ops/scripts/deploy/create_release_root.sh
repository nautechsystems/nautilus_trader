#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)"
# shellcheck source=/dev/null
source "${ROOT_DIR}/ops/scripts/deploy/shared_strategy_stack.sh"

RELEASES_ROOT="${RELEASES_ROOT:-${HOME}/releases}"
DEPLOY_LANE="${DEPLOY_LANE:?DEPLOY_LANE is required}"
STACK_NAME="${STACK_NAME:?STACK_NAME is required}"
SOURCE_ROOT="${SOURCE_ROOT:?SOURCE_ROOT is required}"
RELEASE_ID="${RELEASE_ID:-$(date -u +%Y%m%dT%H%M%SZ)}"
SOURCE_REF="${SOURCE_REF:-unknown}"

strategy_stack_require_lane "${DEPLOY_LANE}"
strategy_stack_require_identifier "${STACK_NAME}" "stack"
strategy_stack_require_identifier "${RELEASE_ID}" "release ID"

[[ -d "${SOURCE_ROOT}" ]] || {
  echo "[create-release-root] source root missing or not a directory: ${SOURCE_ROOT}" >&2
  exit 1
}

RELEASE_ROOT="${RELEASES_ROOT}/${DEPLOY_LANE}/${STACK_NAME}/releases/${RELEASE_ID}"
CURRENT_LINK="${RELEASES_ROOT}/${DEPLOY_LANE}/${STACK_NAME}/current"

install -d "${RELEASE_ROOT}"

rsync -a \
  --delete \
  --exclude '.git' \
  --exclude '.worktrees' \
  --exclude '.venv' \
  --exclude 'target' \
  --exclude 'build' \
  --exclude 'node_modules' \
  "${SOURCE_ROOT}/" "${RELEASE_ROOT}/"

strategy_stack_write_release_metadata \
  "${RELEASE_ROOT}" \
  "${DEPLOY_LANE}" \
  "${STACK_NAME}" \
  "${RELEASE_ID}" \
  "${SOURCE_ROOT}" \
  "${SOURCE_REF}"

ln -sfn "${RELEASE_ROOT}" "${CURRENT_LINK}"
printf '%s\n' "${RELEASE_ROOT}"
