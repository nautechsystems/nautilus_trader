#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "${ROOT_DIR}/ops/scripts/deploy/shared_strategy_stack.sh"
RUN_DIR="${RUN_DIR:-${ROOT_DIR}/.run/equities-stack}"
PID_DIR="${RUN_DIR}/pids"
LOG_DIR="${RUN_DIR}/logs"
DEFAULT_ENV_PATH="${ROOT_DIR}/deploy/equities/equities_stack.env"
CONFIG_PATH="${EQUITIES_CONFIG_PATH:-${ROOT_DIR}/deploy/equities/equities.live.toml}"
STRATEGIES_DIR="${EQUITIES_STRATEGIES_DIR:-${ROOT_DIR}/deploy/equities/strategies}"
ENV_PATH="${EQUITIES_ENV_PATH:-${DEFAULT_ENV_PATH}}"
MODE="${EQUITIES_MODE:-paper}"
CONFIRM_LIVE="${EQUITIES_CONFIRM_LIVE:-0}"
ENABLE_EXECUTION="${EQUITIES_ENABLE_EXECUTION:-0}"
ALLOW_MISSING_KEYS="${EQUITIES_ALLOW_MISSING_KEYS:-0}"
API_HOST="${EQUITIES_API_HOST:-127.0.0.1}"
API_PORT="${EQUITIES_API_PORT:-5022}"
SKIP_FLUXBOARD_BUILD="${EQUITIES_SKIP_FLUXBOARD_BUILD:-0}"
SKIP_PULSE_BUILD="${EQUITIES_SKIP_PULSE_BUILD:-0}"
LOAD_AWS_SECRETS="${EQUITIES_LOAD_AWS_SECRETS:-0}"
AWS_REGION="${EQUITIES_AWS_REGION:-ap-southeast-1}"
TRADE_XYZ_SECRET_ID="${EQUITIES_TRADE_XYZ_SECRET_ID:-/nautilus/equities/trade_xyz}"

is_allowed_env_key() {
  local key="$1"
  case "${key}" in
    EQUITIES_* | TRADE_XYZ_*)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}


usage() {
  cat <<USAGE
Usage: ops/scripts/deploy/equities_stack.sh <start|stop|restart|status|health|logs <svc>>
USAGE
}

ensure_dirs() {
  mkdir -p "${PID_DIR}" "${LOG_DIR}"
}

load_env_file() {
  if [[ -n "${EQUITIES_ENV_PATH:-}" && ! -f "${ENV_PATH}" ]]; then
    echo "[equities-stack] env file not found: ${ENV_PATH}" >&2
    exit 1
  fi

  if [[ -f "${ENV_PATH}" ]]; then
    while IFS= read -r line || [[ -n "${line}" ]]; do
      line="${line#"${line%%[![:space:]]*}"}"
      line="${line%"${line##*[![:space:]]}"}"
      [[ -z "${line}" ]] && continue
      [[ "${line}" == \#* ]] && continue
      if [[ "${line}" == export\ * ]]; then
        line="${line#export }"
        line="${line#"${line%%[![:space:]]*}"}"
      fi

      if [[ "${line}" =~ ^([A-Za-z_][A-Za-z0-9_]*)=(.*)$ ]]; then
        local key="${BASH_REMATCH[1]}"
        local value="${BASH_REMATCH[2]}"
        if ! is_allowed_env_key "${key}"; then
          echo "[equities-stack] invalid key in env file ${ENV_PATH}: ${key}" >&2
          exit 1
        fi
        if [[ "${value}" == \"*\" && "${value}" == *\" ]]; then
          value="${value:1:-1}"
        elif [[ "${value}" == \'*\' && "${value}" == *\' ]]; then
          value="${value:1:-1}"
        fi
        printf -v "${key}" '%s' "${value}"
        export "${key?}"
      else
        echo "[equities-stack] invalid line in env file ${ENV_PATH}: ${line}" >&2
        exit 1
      fi
    done < "${ENV_PATH}"
  fi

  CONFIG_PATH="${EQUITIES_CONFIG_PATH:-${CONFIG_PATH}}"
  STRATEGIES_DIR="${EQUITIES_STRATEGIES_DIR:-${STRATEGIES_DIR}}"
  MODE="${EQUITIES_MODE:-${MODE}}"
  CONFIRM_LIVE="${EQUITIES_CONFIRM_LIVE:-${CONFIRM_LIVE}}"
  ENABLE_EXECUTION="${EQUITIES_ENABLE_EXECUTION:-${ENABLE_EXECUTION}}"
  ALLOW_MISSING_KEYS="${EQUITIES_ALLOW_MISSING_KEYS:-${ALLOW_MISSING_KEYS}}"
  API_HOST="${EQUITIES_API_HOST:-${API_HOST}}"
  API_PORT="${EQUITIES_API_PORT:-${API_PORT}}"
  SKIP_FLUXBOARD_BUILD="${EQUITIES_SKIP_FLUXBOARD_BUILD:-${SKIP_FLUXBOARD_BUILD}}"
  SKIP_PULSE_BUILD="${EQUITIES_SKIP_PULSE_BUILD:-${SKIP_PULSE_BUILD}}"
  LOAD_AWS_SECRETS="${EQUITIES_LOAD_AWS_SECRETS:-${LOAD_AWS_SECRETS}}"
  AWS_REGION="${EQUITIES_AWS_REGION:-${AWS_REGION}}"
  TRADE_XYZ_SECRET_ID="${EQUITIES_TRADE_XYZ_SECRET_ID:-${TRADE_XYZ_SECRET_ID}}"
}

require_cmd() {
  local cmd="$1"
  if ! command -v "${cmd}" > /dev/null 2>&1; then
    echo "[equities-stack] required command not found: ${cmd}" >&2
    exit 1
  fi
}

load_secret_into_env() {
  local secret_id="$1"
  if [[ -z "${secret_id}" ]]; then
    return
  fi
  local raw
  if ! raw="$(aws secretsmanager get-secret-value --region "${AWS_REGION}" --secret-id "${secret_id}" --query SecretString --output text 2> /dev/null)"; then
    echo "[equities-stack] warning: failed to load secret ${secret_id}" >&2
    return
  fi
  if [[ -z "${raw}" || "${raw}" == "None" ]]; then
    echo "[equities-stack] warning: secret ${secret_id} is empty" >&2
    return
  fi

  while IFS='=' read -r key value; do
    [[ -z "${key}" ]] && continue
    if [[ ! "${key}" =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]]; then
      continue
    fi
    case "${key}" in
      TRADE_XYZ_AGENT_PK | TRADE_XYZ_ACCOUNT_ADDRESS)
        printf -v "${key}" "%s" "${value}"
        export "${key?}"
        ;;
      *)
        echo "[equities-stack] warning: skipping unsupported secret key ${key}" >&2
        ;;
    esac
  done < <(printf '%s' "${raw}" | jq -r 'to_entries[] | "\(.key)=\(.value|tostring)"')
}

load_aws_secrets_if_enabled() {
  if [[ "${LOAD_AWS_SECRETS}" != "1" ]]; then
    return
  fi
  require_cmd aws
  require_cmd jq
  load_secret_into_env "${TRADE_XYZ_SECRET_ID}"
}

validate_mode() {
  case "${MODE}" in
    paper|testnet) ;;
    live)
      echo "[equities-stack] live deployments are not supported via equities_stack.sh" >&2
      echo "[equities-stack] install flux@ services with ops/scripts/deploy/install_equities_systemd.sh" >&2
      exit 1
      ;;
    *)
      echo "[equities-stack] invalid EQUITIES_MODE=${MODE}; expected paper|testnet|live" >&2
      exit 1
      ;;
  esac
}

validate_files() {
  [[ -f "${CONFIG_PATH}" ]] || { echo "[equities-stack] config not found: ${CONFIG_PATH}" >&2; exit 1; }
  [[ -d "${STRATEGIES_DIR}" ]] || { echo "[equities-stack] strategies dir not found: ${STRATEGIES_DIR}" >&2; exit 1; }
}

validate_credentials() {
  if [[ "${ALLOW_MISSING_KEYS}" == "1" ]]; then
    return
  fi
  local missing=()
  [[ -z "${TRADE_XYZ_AGENT_PK:-}" ]] && missing+=("TRADE_XYZ_AGENT_PK")
  [[ -z "${TRADE_XYZ_ACCOUNT_ADDRESS:-}" ]] && missing+=("TRADE_XYZ_ACCOUNT_ADDRESS")
  if (( ${#missing[@]} > 0 )); then
    echo "[equities-stack] missing required credentials: ${missing[*]}" >&2
    exit 1
  fi
}

build_ui() {
  if [[ "${SKIP_FLUXBOARD_BUILD}" != "1" ]]; then
    pnpm --dir "${ROOT_DIR}/fluxboard" build >/dev/null 2>&1 || true
  fi
  if [[ "${SKIP_PULSE_BUILD}" != "1" ]]; then
    pnpm --dir "${ROOT_DIR}/pulse-ui" build >/dev/null 2>&1 || true
  fi
}

start_process() {
  local name="$1"
  shift
  local pid_file="${PID_DIR}/${name}.pid"
  local log_file="${LOG_DIR}/${name}.log"
  if [[ -f "${pid_file}" ]] && kill -0 "$(cat "${pid_file}")" 2>/dev/null; then
    return
  fi
  nohup "$@" >>"${log_file}" 2>&1 < /dev/null &
  echo $! > "${pid_file}"
}

stop_process() {
  local name="$1"
  local pid_file="${PID_DIR}/${name}.pid"
  if [[ -f "${pid_file}" ]]; then
    kill "$(cat "${pid_file}")" 2>/dev/null || true
    rm -f "${pid_file}"
  fi
}

node_configs() {
  strategy_stack_print_strategy_configs "${STRATEGIES_DIR}" "equities.strategy.template.toml"
}

start_stack() {
  ensure_dirs
  load_env_file
  load_aws_secrets_if_enabled
  validate_mode
  validate_files
  validate_credentials
  build_ui
  echo "[equities-stack] runtime intent: mode=${MODE} confirm_live=${CONFIRM_LIVE} enable_execution=${ENABLE_EXECUTION}"
  start_process portfolio python3 -m nautilus_trader.flux.runners.equities.run_portfolio --config "${CONFIG_PATH}" --mode "${MODE}"
  start_process bridge python3 -m nautilus_trader.flux.runners.equities.run_bridge --config "${CONFIG_PATH}" --mode "${MODE}"
  start_process api env FLUXBOARD_SERVE_DIST=1 PULSE_SERVE_DIST=1 python3 -m nautilus_trader.flux.runners.equities.run_api --config "${CONFIG_PATH}" --mode "${MODE}" --host "${API_HOST}" --port "${API_PORT}" --serve-fluxboard --serve-pulse
  local config_path service_name
  while IFS= read -r config_path; do
    service_name="node_$(basename "${config_path}" .toml)"
    start_process "${service_name}" python3 -m nautilus_trader.flux.runners.equities.run_node --config "${config_path}" --shared-config "${CONFIG_PATH}" --mode "${MODE}"
  done < <(node_configs)
}

stop_stack() {
  stop_process api
  stop_process bridge
  stop_process portfolio
  local config_path
  while IFS= read -r config_path; do
    stop_process "node_$(basename "${config_path}" .toml)"
  done < <(node_configs)
}

status_stack() {
  echo "[equities-stack] status"
  ls -1 "${PID_DIR}" 2>/dev/null || true
}

health_stack() {
  local base_url="http://${API_HOST}:${API_PORT}"
  curl -fsS "${base_url}/api/v1/healthz" >/dev/null
  curl -fsS "${base_url}/equities" >/dev/null
  curl -fsS "${base_url}/api/v1/params?profile=equities" >/dev/null
  curl -fsS "${base_url}/api/v1/balances?profile=equities" >/dev/null
  curl -fsS "${base_url}/api/v1/trades?profile=equities" >/dev/null
}

logs_stack() {
  local service_name="${1:-}"
  if [[ -z "${service_name}" ]]; then
    echo "[equities-stack] logs requires a service name" >&2
    exit 1
  fi
  tail -n 200 -f "${LOG_DIR}/${service_name}.log"
}

main() {
  local cmd="${1:-}"
  case "${cmd}" in
    start) start_stack ;;
    stop) stop_stack ;;
    restart) stop_stack; start_stack ;;
    status) status_stack ;;
    health) health_stack ;;
    logs) logs_stack "${2:-}" ;;
    *) usage; exit 1 ;;
  esac
}

main "$@"
