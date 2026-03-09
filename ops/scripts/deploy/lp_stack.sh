#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)"
RUN_DIR="${RUN_DIR:-${ROOT_DIR}/.run/lp-stack}"
PID_DIR="${RUN_DIR}/pids"
LOG_DIR="${RUN_DIR}/logs"
DEFAULT_ENV_PATH="${ROOT_DIR}/deploy/lp/lp_stack.env"

CONFIG_PATH="${LP_CONFIG_PATH:-${ROOT_DIR}/deploy/lp/lp.live.toml}"
HEDGERS_DIR="${LP_HEDGERS_DIR:-${ROOT_DIR}/deploy/lp/hedgers}"
ENV_PATH="${LP_ENV_PATH:-${DEFAULT_ENV_PATH}}"
LP_MODE="${LP_MODE:-paper}"
LP_CONFIRM_LIVE="${LP_CONFIRM_LIVE:-0}"
LP_ENABLE_EXECUTION="${LP_ENABLE_EXECUTION:-0}"
LP_API_HOST="${LP_API_HOST:-127.0.0.1}"
LP_API_PORT="${LP_API_PORT:-5025}"
PUBLIC_API_HOST="${LP_PUBLIC_HOST:-127.0.0.1}"
PUBLIC_API_PORT="${LP_PUBLIC_PORT:-5022}"
LP_SKIP_FLUXBOARD_BUILD="${LP_SKIP_FLUXBOARD_BUILD:-0}"
LP_SKIP_PULSE_BUILD="${LP_SKIP_PULSE_BUILD:-0}"
PUBLIC_CONFIG="${ROOT_DIR}/deploy/tokenmm/tokenmm.live.toml"

usage() {
  cat << 'USAGE'
Usage: ops/scripts/deploy/lp_stack.sh <start|stop|restart|status|health|logs <svc>>
USAGE
}

is_allowed_env_key() {
  local key="$1"
  case "${key}" in
    LP_*)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

ensure_dirs() {
  mkdir -p "${PID_DIR}" "${LOG_DIR}"
}

load_env_file() {
  if [[ -n "${LP_ENV_PATH:-}" && ! -f "${ENV_PATH}" ]]; then
    echo "[lp-stack] env file not found: ${ENV_PATH}" >&2
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
          echo "[lp-stack] invalid key in env file ${ENV_PATH}: ${key}" >&2
          exit 1
        fi
        if printenv "${key}" > /dev/null 2>&1; then
          continue
        fi
        if [[ "${value}" == \"*\" && "${value}" == *\" ]]; then
          value="${value:1:-1}"
        elif [[ "${value}" == \'*\' && "${value}" == *\' ]]; then
          value="${value:1:-1}"
        fi
        printf -v "${key}" '%s' "${value}"
        export "${key?}"
      else
        echo "[lp-stack] invalid line in env file ${ENV_PATH}: ${line}" >&2
        exit 1
      fi
    done < "${ENV_PATH}"
  fi

  CONFIG_PATH="${LP_CONFIG_PATH:-${CONFIG_PATH}}"
  HEDGERS_DIR="${LP_HEDGERS_DIR:-${HEDGERS_DIR}}"
  LP_MODE="${LP_MODE:-${LP_MODE}}"
  LP_CONFIRM_LIVE="${LP_CONFIRM_LIVE:-${LP_CONFIRM_LIVE}}"
  LP_ENABLE_EXECUTION="${LP_ENABLE_EXECUTION:-${LP_ENABLE_EXECUTION}}"
  LP_API_HOST="${LP_API_HOST:-${LP_API_HOST}}"
  LP_API_PORT="${LP_API_PORT:-${LP_API_PORT}}"
  PUBLIC_API_HOST="${LP_PUBLIC_HOST:-${PUBLIC_API_HOST}}"
  PUBLIC_API_PORT="${LP_PUBLIC_PORT:-${PUBLIC_API_PORT}}"
  LP_SKIP_FLUXBOARD_BUILD="${LP_SKIP_FLUXBOARD_BUILD:-${LP_SKIP_FLUXBOARD_BUILD}}"
  LP_SKIP_PULSE_BUILD="${LP_SKIP_PULSE_BUILD:-${LP_SKIP_PULSE_BUILD}}"
}

validate_mode() {
  case "${LP_MODE}" in
    paper | testnet) ;;
    live)
      if [[ "${LP_CONFIRM_LIVE}" != "1" || "${LP_ENABLE_EXECUTION}" != "1" ]]; then
        echo "[lp-stack] live mode requires LP_CONFIRM_LIVE=1 and LP_ENABLE_EXECUTION=1" >&2
        exit 1
      fi
      ;;
    *)
      echo "[lp-stack] invalid LP_MODE=${LP_MODE}; expected paper|testnet|live" >&2
      exit 1
      ;;
  esac
}

build_ui() {
  if [[ "${LP_SKIP_FLUXBOARD_BUILD}" != "1" ]] && command -v pnpm > /dev/null 2>&1; then
    pnpm --dir "${ROOT_DIR}/fluxboard" build > /dev/null 2>&1 || true
  fi
  if [[ "${LP_SKIP_PULSE_BUILD}" != "1" ]] && command -v pnpm > /dev/null 2>&1; then
    pnpm --dir "${ROOT_DIR}/pulse-ui" build > /dev/null 2>&1 || true
  fi
}

pid_path() {
  printf '%s/%s.pid\n' "${PID_DIR}" "$1"
}

log_path() {
  printf '%s/%s.log\n' "${LOG_DIR}" "$1"
}

is_running() {
  local name="$1"
  local pid_file
  pid_file="$(pid_path "${name}")"
  [[ -f "${pid_file}" ]] || return 1
  local pid
  pid="$(< "${pid_file}")"
  kill -0 "${pid}" > /dev/null 2>&1
}

start_service() {
  local name="$1"
  local cmd="$2"
  local pid_file log_file
  pid_file="$(pid_path "${name}")"
  log_file="$(log_path "${name}")"
  if is_running "${name}"; then
    echo "[lp-stack] ${name} already running"
    return
  fi
  (
    cd "${ROOT_DIR}"
    nohup /bin/bash -lc "${cmd}" >> "${log_file}" 2>&1 &
    echo "$!" > "${pid_file}"
  )
  echo "[lp-stack] started ${name}"
}

stop_service() {
  local name="$1"
  local pid_file
  pid_file="$(pid_path "${name}")"
  [[ -f "${pid_file}" ]] || return 0
  local pid
  pid="$(< "${pid_file}")"
  if kill -0 "${pid}" > /dev/null 2>&1; then
    kill "${pid}" > /dev/null 2>&1 || true
    wait "${pid}" 2> /dev/null || true
  fi
  rm -f "${pid_file}"
  echo "[lp-stack] stopped ${name}"
}

start_stack() {
  ensure_dirs
  load_env_file
  validate_mode
  build_ui

  local confirm_live_arg=""
  if [[ "${LP_MODE}" == "live" && "${LP_CONFIRM_LIVE}" == "1" ]]; then
    confirm_live_arg=" --confirm-live"
  fi

  start_service \
    "lp-api" \
    "python3 -m lp.runners.run_api --config ${CONFIG_PATH} --host ${LP_API_HOST} --port ${LP_API_PORT} --serve-fluxboard"

  start_service \
    "public-api" \
    "env LP_API_BACKEND_URL=\"http://${LP_API_HOST}:${LP_API_PORT}\" FLUXBOARD_SERVE_DIST=1 PULSE_SERVE_DIST=1 python3 -m nautilus_trader.flux.runners.tokenmm.run_api --config ${PUBLIC_CONFIG} --mode ${LP_MODE}${confirm_live_arg} --host ${PUBLIC_API_HOST} --port ${PUBLIC_API_PORT} --serve-fluxboard --serve-pulse"
}

stop_stack() {
  stop_service "public-api"
  stop_service "lp-api"
}

status_stack() {
  ensure_dirs
  for service_name in lp-api public-api; do
    if is_running "${service_name}"; then
      echo "${service_name}: running"
    else
      echo "${service_name}: stopped"
    fi
  done
}

health_stack() {
  load_env_file
  curl -fsS "http://${PUBLIC_API_HOST}:${PUBLIC_API_PORT}/lp" > /dev/null
  curl -fsS "http://${PUBLIC_API_HOST}:${PUBLIC_API_PORT}/api/v1/hedgers/instances" > /dev/null
  echo "[lp-stack] health checks passed"
}

show_logs() {
  local service_name="${1:-}"
  if [[ -z "${service_name}" ]]; then
    echo "[lp-stack] logs requires <lp-api|public-api>" >&2
    exit 1
  fi
  tail -n 200 "$(log_path "${service_name}")"
}

main() {
  local command="${1:-}"
  case "${command}" in
    start)
      start_stack
      ;;
    stop)
      stop_stack
      ;;
    restart)
      stop_stack
      start_stack
      ;;
    status)
      status_stack
      ;;
    health)
      health_stack
      ;;
    logs)
      show_logs "${2:-}"
      ;;
    *)
      usage
      exit 1
      ;;
  esac
}

main "$@"
