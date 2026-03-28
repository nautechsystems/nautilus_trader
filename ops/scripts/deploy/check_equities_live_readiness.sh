#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
PYTHON_BIN="${ROOT_DIR}/.venv/bin/python"
EQUITIES_READINESS_COMMON_ENV_PATH="${EQUITIES_READINESS_COMMON_ENV_PATH:-/etc/flux/common.env}"

load_common_env() {
  local env_path="$1"
  if [[ ! -f "${env_path}" ]]; then
    return 0
  fi

  if [[ -r "${env_path}" ]]; then
    set -a
    # shellcheck disable=SC1090
    source "${env_path}"
    set +a
    return 0
  fi

  if ! command -v sudo >/dev/null 2>&1 || ! sudo -n test -r "${env_path}" 2>/dev/null; then
    return 0
  fi

  while IFS= read -r line; do
    [[ -n "${line}" ]] || continue
    export "${line}"
  done < <(
    sudo -n sed -n \
      -e '/^EQUITIES_REDIS_HOST=/p' \
      -e '/^EQUITIES_REDIS_PORT=/p' \
      -e '/^EQUITIES_REDIS_DB=/p' \
      -e '/^EQUITIES_REDIS_USERNAME=/p' \
      -e '/^EQUITIES_REDIS_PASSWORD=/p' \
      -e '/^EQUITIES_REDIS_SSL=/p' \
      -e '/^EQUITIES_API_BACKEND_URL=/p' \
      "${env_path}"
  )
}

load_common_env "${EQUITIES_READINESS_COMMON_ENV_PATH}"

EQUITIES_READINESS_CONFIG_PATH="${EQUITIES_READINESS_CONFIG_PATH:-${ROOT_DIR}/deploy/equities/equities.live.toml}"
EQUITIES_READINESS_API_BASE_URL="${EQUITIES_READINESS_API_BASE_URL:-${EQUITIES_API_BACKEND_URL:-}}"
EQUITIES_READY_MAX_STALE_SIGNAL_LEGS="${EQUITIES_READY_MAX_STALE_SIGNAL_LEGS:-0}"
EQUITIES_READY_MAX_UNHEALTHY_STRATEGIES="${EQUITIES_READY_MAX_UNHEALTHY_STRATEGIES:-0}"
EQUITIES_READY_PROJECTION_MAX_AGE_MS="${EQUITIES_READY_PROJECTION_MAX_AGE_MS:-120000}"
EQUITIES_READY_REQUIRED_BALANCE_SOURCE="${EQUITIES_READY_REQUIRED_BALANCE_SOURCE:-portfolio_snapshot_v2}"
EQUITIES_READY_IGNORE_REFERENCE_FRESHNESS_OUTSIDE_REGULAR_SESSION="${EQUITIES_READY_IGNORE_REFERENCE_FRESHNESS_OUTSIDE_REGULAR_SESSION:-1}"
EQUITIES_READY_EXPECTED_PROJECTION_SCOPE_IDS="${EQUITIES_READY_EXPECTED_PROJECTION_SCOPE_IDS:-}"

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

if [[ -n "${EQUITIES_READY_EXPECTED_PROJECTION_SCOPE_IDS}" ]]; then
  IFS=',' read -r -a expected_scope_ids <<<"${EQUITIES_READY_EXPECTED_PROJECTION_SCOPE_IDS}"
  for scope_id in "${expected_scope_ids[@]}"; do
    scope_id="${scope_id//[[:space:]]/}"
    if [[ -n "${scope_id}" ]]; then
      cmd+=(--expected-projection-scope-id "${scope_id}")
    fi
  done
fi

case "${EQUITIES_READY_IGNORE_REFERENCE_FRESHNESS_OUTSIDE_REGULAR_SESSION,,}" in
  1|true|yes|on)
    cmd+=(--ignore-reference-freshness-outside-regular-session)
    ;;
esac

exec "${cmd[@]}" "$@"
