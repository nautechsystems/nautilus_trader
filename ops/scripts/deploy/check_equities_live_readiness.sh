#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
PYTHON_BIN="${ROOT_DIR}/.venv/bin/python"
EQUITIES_READINESS_CONFIG_PATH="${EQUITIES_READINESS_CONFIG_PATH:-${ROOT_DIR}/deploy/equities/equities.live.toml}"
EQUITIES_READINESS_API_BASE_URL="${EQUITIES_READINESS_API_BASE_URL:-${EQUITIES_API_BACKEND_URL:-}}"
EQUITIES_READY_MAX_STALE_SIGNAL_LEGS="${EQUITIES_READY_MAX_STALE_SIGNAL_LEGS:-0}"
EQUITIES_READY_MAX_UNHEALTHY_STRATEGIES="${EQUITIES_READY_MAX_UNHEALTHY_STRATEGIES:-0}"
EQUITIES_READY_PROJECTION_MAX_AGE_MS="${EQUITIES_READY_PROJECTION_MAX_AGE_MS:-120000}"
EQUITIES_READY_REQUIRED_BALANCE_SOURCE="${EQUITIES_READY_REQUIRED_BALANCE_SOURCE:-portfolio_snapshot_v2}"

if [[ ! -x "${PYTHON_BIN}" ]]; then
  echo "[equities-readiness] missing checkout-local python: ${PYTHON_BIN}" >&2
  exit 1
fi

cmd=(
  "${PYTHON_BIN}"
  -m nautilus_trader.flux.runners.equities.readiness
  --config "${EQUITIES_READINESS_CONFIG_PATH}"
  --max-stale-signal-legs "${EQUITIES_READY_MAX_STALE_SIGNAL_LEGS}"
  --max-unhealthy-strategies "${EQUITIES_READY_MAX_UNHEALTHY_STRATEGIES}"
  --projection-max-age-ms "${EQUITIES_READY_PROJECTION_MAX_AGE_MS}"
  --required-balance-source "${EQUITIES_READY_REQUIRED_BALANCE_SOURCE}"
)

if [[ -n "${EQUITIES_READINESS_API_BASE_URL}" ]]; then
  cmd+=(--api-base-url "${EQUITIES_READINESS_API_BASE_URL}")
fi

exec "${cmd[@]}" "$@"
