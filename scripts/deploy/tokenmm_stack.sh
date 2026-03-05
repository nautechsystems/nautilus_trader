#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
RUN_DIR="${RUN_DIR:-${ROOT_DIR}/.run/tokenmm-stack}"
LOG_DIR="${RUN_DIR}/logs"
PID_DIR="${RUN_DIR}/pids"

CONFIG_PATH="${TOKENMM_CONFIG_PATH:-${ROOT_DIR}/examples/live/makerv3/config/makerv3.live.toml}"
STRATEGIES_DIR="${TOKENMM_STRATEGIES_DIR:-${ROOT_DIR}/examples/live/makerv3/config/strategies.d}"
ENV_PATH="${TOKENMM_ENV_PATH:-${ROOT_DIR}/examples/live/makerv3/config/makerv3.live.env}"
MODE="${TOKENMM_MODE:-paper}"
CONFIRM_LIVE="${TOKENMM_CONFIRM_LIVE:-0}"
ENABLE_EXECUTION="${TOKENMM_ENABLE_EXECUTION:-0}"
ALLOW_MISSING_KEYS="${TOKENMM_ALLOW_MISSING_KEYS:-0}"
MANAGE_REDIS="${TOKENMM_MANAGE_REDIS:-1}"
API_HOST="${TOKENMM_API_HOST:-127.0.0.1}"
API_PORT="${TOKENMM_API_PORT:-5022}"
EXPECTED_NODES="${TOKENMM_EXPECTED_NODES:-5}"
START_TIMEOUT_SECS="${TOKENMM_START_TIMEOUT_SECS:-90}"
SKIP_FLUXBOARD_BUILD="${TOKENMM_SKIP_FLUXBOARD_BUILD:-0}"
PYTHON_BIN="${TOKENMM_PYTHON_BIN:-}"

REDIS_HOST="127.0.0.1"
REDIS_PORT="6380"

declare -a STRATEGY_CONFIGS=()
readonly ROOT_DIR RUN_DIR LOG_DIR PID_DIR

usage() {
  cat << 'USAGE'
Usage: scripts/deploy/tokenmm_stack.sh <command>

Commands:
  start      Build and start redis (optional), N nodes, 1 bridge, and 1 API + Fluxboard
  stop       Stop API, bridge, all node processes, and managed redis process
  restart    Stop then start
  status     Show process and endpoint status
  health     Run health checks for API, tokenmm route, and socket handshake
  logs <svc> Tail service log (svc: redis|bridge|api|node_<strategy_slug>)

Environment overrides:
  TOKENMM_ENV_PATH, TOKENMM_CONFIG_PATH, TOKENMM_STRATEGIES_DIR
  TOKENMM_MODE, TOKENMM_CONFIRM_LIVE, TOKENMM_ENABLE_EXECUTION
  TOKENMM_ALLOW_MISSING_KEYS, TOKENMM_MANAGE_REDIS
  TOKENMM_API_HOST, TOKENMM_API_PORT, TOKENMM_EXPECTED_NODES
  TOKENMM_SKIP_FLUXBOARD_BUILD, TOKENMM_START_TIMEOUT_SECS
USAGE
}

require_cmd() {
  local cmd="$1"
  if ! command -v "${cmd}" > /dev/null 2>&1; then
    echo "[tokenmm-stack] required command not found: ${cmd}" >&2
    exit 1
  fi
}

is_allowed_env_key() {
  local key="$1"
  case "${key}" in
    TOKENMM_* | BYBIT_* | BINANCE_*)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

resolve_python_bin() {
  if [[ -n "${PYTHON_BIN}" ]]; then
    require_cmd "${PYTHON_BIN}"
    echo "${PYTHON_BIN}"
    return
  fi
  if command -v python3 > /dev/null 2>&1; then
    echo "python3"
    return
  fi
  if command -v python > /dev/null 2>&1; then
    echo "python"
    return
  fi
  echo "[tokenmm-stack] required command not found: python3/python" >&2
  exit 1
}

load_env_file() {
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
          echo "[tokenmm-stack] invalid key in env file ${ENV_PATH}: ${key}" >&2
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
        echo "[tokenmm-stack] invalid line in env file ${ENV_PATH}: ${line}" >&2
        exit 1
      fi
    done < "${ENV_PATH}"
  fi

  CONFIG_PATH="${TOKENMM_CONFIG_PATH:-${CONFIG_PATH}}"
  STRATEGIES_DIR="${TOKENMM_STRATEGIES_DIR:-${STRATEGIES_DIR}}"
  MODE="${TOKENMM_MODE:-${MODE}}"
  CONFIRM_LIVE="${TOKENMM_CONFIRM_LIVE:-${CONFIRM_LIVE}}"
  ENABLE_EXECUTION="${TOKENMM_ENABLE_EXECUTION:-${ENABLE_EXECUTION}}"
  ALLOW_MISSING_KEYS="${TOKENMM_ALLOW_MISSING_KEYS:-${ALLOW_MISSING_KEYS}}"
  MANAGE_REDIS="${TOKENMM_MANAGE_REDIS:-${MANAGE_REDIS}}"
  API_HOST="${TOKENMM_API_HOST:-${API_HOST}}"
  API_PORT="${TOKENMM_API_PORT:-${API_PORT}}"
  EXPECTED_NODES="${TOKENMM_EXPECTED_NODES:-${EXPECTED_NODES}}"
  START_TIMEOUT_SECS="${TOKENMM_START_TIMEOUT_SECS:-${START_TIMEOUT_SECS}}"
  SKIP_FLUXBOARD_BUILD="${TOKENMM_SKIP_FLUXBOARD_BUILD:-${SKIP_FLUXBOARD_BUILD}}"
}

resolve_redis_target_from_config() {
  local pybin="$1"
  if [[ ! -f "${CONFIG_PATH}" ]]; then
    echo "[tokenmm-stack] config not found: ${CONFIG_PATH}" >&2
    exit 1
  fi

  local output
  output="$(
    "${pybin}" - "${CONFIG_PATH}" << 'PY'
import sys
from pathlib import Path
import tomllib

path = Path(sys.argv[1])
data = tomllib.load(path.open("rb"))
redis_cfg = data.get("redis") or {}
host = str(redis_cfg.get("host", "127.0.0.1")).strip() or "127.0.0.1"
port = int(redis_cfg.get("port", 6380))
print(f"{host} {port}")
PY
  )"
  read -r REDIS_HOST REDIS_PORT <<< "${output}"
}

validate_mode() {
  if [[ "${MODE}" != "paper" && "${MODE}" != "testnet" && "${MODE}" != "live" ]]; then
    echo "[tokenmm-stack] invalid TOKENMM_MODE=${MODE}; expected paper|testnet|live" >&2
    exit 1
  fi
  if [[ "${MODE}" == "live" && "${CONFIRM_LIVE}" != "1" ]]; then
    echo "[tokenmm-stack] refusing live startup: set TOKENMM_CONFIRM_LIVE=1" >&2
    exit 1
  fi
}

validate_config_and_keys() {
  if [[ ! -f "${CONFIG_PATH}" ]]; then
    echo "[tokenmm-stack] config not found: ${CONFIG_PATH}" >&2
    exit 1
  fi
  if [[ ! -d "${STRATEGIES_DIR}" ]]; then
    echo "[tokenmm-stack] strategies dir not found: ${STRATEGIES_DIR}" >&2
    exit 1
  fi

  if [[ "${MODE}" != "live" || "${ALLOW_MISSING_KEYS}" == "1" ]]; then
    return
  fi

  local missing=()
  [[ -z "${BYBIT_API_KEY:-}" ]] && missing+=("BYBIT_API_KEY")
  [[ -z "${BYBIT_API_SECRET:-}" ]] && missing+=("BYBIT_API_SECRET")
  [[ -z "${BINANCE_API_KEY:-}" ]] && missing+=("BINANCE_API_KEY")
  [[ -z "${BINANCE_API_SECRET:-}" ]] && missing+=("BINANCE_API_SECRET")
  if ((${#missing[@]} > 0)); then
    echo "[tokenmm-stack] missing required live credentials: ${missing[*]}" >&2
    echo "[tokenmm-stack] set TOKENMM_ALLOW_MISSING_KEYS=1 only for market-data smoke." >&2
    exit 1
  fi
}

load_strategy_configs() {
  mapfile -t STRATEGY_CONFIGS < <(find "${STRATEGIES_DIR}" -maxdepth 1 -type f -name '*.toml' ! -name '*template*' | sort)
  if ((${#STRATEGY_CONFIGS[@]} == 0)); then
    echo "[tokenmm-stack] no strategy configs found under ${STRATEGIES_DIR}" >&2
    exit 1
  fi
  if [[ "${EXPECTED_NODES}" =~ ^[0-9]+$ ]] && [[ "${EXPECTED_NODES}" != "0" ]]; then
    if ((${#STRATEGY_CONFIGS[@]} != EXPECTED_NODES)); then
      echo "[tokenmm-stack] expected ${EXPECTED_NODES} strategy configs, found ${#STRATEGY_CONFIGS[@]}" >&2
      exit 1
    fi
  fi
}

strategy_flux_id() {
  local pybin="$1"
  local config_path="$2"
  "${pybin}" - "${config_path}" << 'PY'
import sys
from pathlib import Path
import tomllib

data = tomllib.load(Path(sys.argv[1]).open("rb"))
identity = data.get("identity") or {}
strategy_id = str(identity.get("strategy_id", "")).strip()
if not strategy_id:
    raise SystemExit("missing [identity].strategy_id")
print(strategy_id)
PY
}

slugify() {
  local text="$1"
  text="${text//[^A-Za-z0-9_]/_}"
  echo "${text}"
}

pid_file() {
  local svc="$1"
  echo "${PID_DIR}/${svc}.pid"
}

log_file() {
  local svc="$1"
  echo "${LOG_DIR}/${svc}.log"
}

ensure_dirs() {
  mkdir -p "${LOG_DIR}" "${PID_DIR}"
}

is_running() {
  local svc="$1"
  local file
  file="$(pid_file "${svc}")"
  if [[ ! -f "${file}" ]]; then
    return 1
  fi
  local pid
  pid="$(cat "${file}")"
  [[ -n "${pid}" ]] || return 1
  kill -0 "${pid}" > /dev/null 2>&1
}

start_process() {
  local svc="$1"
  shift
  local file
  file="$(pid_file "${svc}")"
  local log
  log="$(log_file "${svc}")"

  if is_running "${svc}"; then
    echo "[tokenmm-stack] ${svc} already running (pid $(cat "${file}"))"
    return
  fi

  echo "[tokenmm-stack] starting ${svc}"
  rm -f "${file}"
  (
    cd "${ROOT_DIR}"
    if command -v nohup > /dev/null 2>&1; then
      nohup "$@" >> "${log}" 2>&1 &
    else
      "$@" >> "${log}" 2>&1 &
    fi
    echo $! > "${file}"
  )
  sleep 1

  local pid
  pid="$(cat "${file}")"
  if ! kill -0 "${pid}" > /dev/null 2>&1; then
    echo "[tokenmm-stack] ${svc} failed to start; log tail:" >&2
    tail -n 80 "${log}" >&2 || true
    exit 1
  fi
}

stop_process() {
  local svc="$1"
  local file
  file="$(pid_file "${svc}")"
  [[ -f "${file}" ]] || return 0

  local pid
  pid="$(cat "${file}")"
  if [[ -n "${pid}" ]] && kill -0 "${pid}" > /dev/null 2>&1; then
    echo "[tokenmm-stack] stopping ${svc} (pid ${pid})"
    kill "${pid}" > /dev/null 2>&1 || true
    for _ in {1..20}; do
      if ! kill -0 "${pid}" > /dev/null 2>&1; then
        break
      fi
      sleep 0.5
    done
    if kill -0 "${pid}" > /dev/null 2>&1; then
      echo "[tokenmm-stack] force-killing ${svc} (pid ${pid})"
      kill -9 "${pid}" > /dev/null 2>&1 || true
    fi
  fi
  rm -f "${file}"
}

wait_for_url() {
  local url="$1"
  local label="$2"
  local deadline=$((SECONDS + START_TIMEOUT_SECS))
  while ((SECONDS < deadline)); do
    if curl -fsS "${url}" > /dev/null 2>&1; then
      echo "[tokenmm-stack] ${label} ready: ${url}"
      return
    fi
    sleep 1
  done
  echo "[tokenmm-stack] timeout waiting for ${label}: ${url}" >&2
  exit 1
}

start_redis_if_needed() {
  require_cmd redis-cli
  if redis-cli -h "${REDIS_HOST}" -p "${REDIS_PORT}" ping > /dev/null 2>&1; then
    echo "[tokenmm-stack] redis already reachable at ${REDIS_HOST}:${REDIS_PORT}"
    return
  fi
  if [[ "${MANAGE_REDIS}" != "1" ]]; then
    echo "[tokenmm-stack] redis not reachable at ${REDIS_HOST}:${REDIS_PORT}" >&2
    echo "[tokenmm-stack] start redis manually or set TOKENMM_MANAGE_REDIS=1" >&2
    exit 1
  fi
  if [[ "${REDIS_HOST}" != "127.0.0.1" && "${REDIS_HOST}" != "localhost" ]]; then
    echo "[tokenmm-stack] refusing to manage redis for non-local host ${REDIS_HOST}:${REDIS_PORT}" >&2
    exit 1
  fi
  require_cmd redis-server
  start_process "redis" redis-server --bind "${REDIS_HOST}" --port "${REDIS_PORT}" --save "" --appendonly no
}

build_fluxboard() {
  if [[ "${SKIP_FLUXBOARD_BUILD}" == "1" ]]; then
    echo "[tokenmm-stack] skipping fluxboard build (TOKENMM_SKIP_FLUXBOARD_BUILD=1)"
    return
  fi
  require_cmd pnpm
  if [[ ! -d "${ROOT_DIR}/fluxboard/node_modules" ]]; then
    echo "[tokenmm-stack] installing fluxboard dependencies"
    pnpm --dir "${ROOT_DIR}/fluxboard" install --frozen-lockfile
  fi
  echo "[tokenmm-stack] building fluxboard"
  pnpm --dir "${ROOT_DIR}/fluxboard" build
}

start_nodes() {
  local pybin="$1"
  local live_flag=""
  local exec_flag=""
  if [[ "${MODE}" == "live" ]]; then
    live_flag="--confirm-live"
  fi
  if [[ "${ENABLE_EXECUTION}" == "1" ]]; then
    exec_flag="--enable-execution"
  fi

  for config_path in "${STRATEGY_CONFIGS[@]}"; do
    local strategy_flux_id
    strategy_flux_id="$(strategy_flux_id "${pybin}" "${config_path}")"
    local svc="node_$(slugify "${strategy_flux_id}")"
    echo "[tokenmm-stack] node config=${config_path} flux_strategy_id=${strategy_flux_id} service=${svc}"
    local -a cmd=(env "PYTHONPATH=${ROOT_DIR}:${PYTHONPATH:-}" "${pybin}" examples/live/makerv3/run_node.py --config "${config_path}" --mode "${MODE}")
    if [[ -n "${live_flag}" ]]; then
      cmd+=("${live_flag}")
    fi
    if [[ -n "${exec_flag}" ]]; then
      cmd+=("${exec_flag}")
    fi
    start_process "${svc}" "${cmd[@]}"
  done
}

start_stack() {
  require_cmd curl
  local pybin
  pybin="$(resolve_python_bin)"

  load_env_file
  validate_mode
  validate_config_and_keys
  load_strategy_configs
  ensure_dirs

  resolve_redis_target_from_config "${pybin}"
  start_redis_if_needed
  build_fluxboard
  start_nodes "${pybin}"

  local live_flag=""
  if [[ "${MODE}" == "live" ]]; then
    live_flag="--confirm-live"
  fi

  local -a bridge_cmd=(env "PYTHONPATH=${ROOT_DIR}:${PYTHONPATH:-}" "${pybin}" examples/live/makerv3/run_bridge.py --config "${CONFIG_PATH}" --mode "${MODE}" --all-strategies)
  if [[ -n "${live_flag}" ]]; then
    bridge_cmd+=("${live_flag}")
  fi
  start_process "bridge" "${bridge_cmd[@]}"

  local -a api_cmd=(env "PYTHONPATH=${ROOT_DIR}:${PYTHONPATH:-}" "${pybin}" examples/live/makerv3/run_api.py --config "${CONFIG_PATH}" --mode "${MODE}")
  if [[ -n "${live_flag}" ]]; then
    api_cmd+=("${live_flag}")
  fi
  api_cmd+=(--host "${API_HOST}" --port "${API_PORT}" --serve-fluxboard)
  start_process "api" "${api_cmd[@]}"

  wait_for_url "http://${API_HOST}:${API_PORT}/api/v1/healthz" "flux api"
  wait_for_url "http://${API_HOST}:${API_PORT}/tokenmm" "fluxboard tokenmm"
  echo "[tokenmm-stack] stack started"
  status_stack
}

stop_nodes() {
  shopt -s nullglob
  local files=("${PID_DIR}"/node_*.pid)
  shopt -u nullglob
  for pid_path in "${files[@]}"; do
    local svc
    svc="$(basename "${pid_path}" .pid)"
    stop_process "${svc}"
  done
}

stop_stack() {
  stop_process "api"
  stop_process "bridge"
  stop_nodes
  stop_process "redis"
  echo "[tokenmm-stack] stack stopped"
}

service_status_line() {
  local svc="$1"
  local state="stopped"
  local pid="-"
  local file
  file="$(pid_file "${svc}")"
  if is_running "${svc}"; then
    state="running"
    pid="$(cat "${file}")"
  elif [[ -f "${file}" ]]; then
    state="stale-pid"
    pid="$(cat "${file}")"
  fi
  printf '%-32s %-10s pid=%s log=%s\n' "${svc}" "${state}" "${pid}" "$(log_file "${svc}")"
}

status_stack() {
  load_env_file
  echo "[tokenmm-stack] status"
  service_status_line "redis"

  shopt -s nullglob
  local files=("${PID_DIR}"/node_*.pid)
  shopt -u nullglob
  if ((${#files[@]} == 0)); then
    echo "node_*                           stopped    pid=- log=${LOG_DIR}/node_*.log"
  else
    for pid_path in "${files[@]}"; do
      local svc
      svc="$(basename "${pid_path}" .pid)"
      service_status_line "${svc}"
    done
  fi

  service_status_line "bridge"
  service_status_line "api"
  echo "[tokenmm-stack] endpoints"
  echo "  API:      http://${API_HOST}:${API_PORT}/api/v1/healthz"
  echo "  TokenMM:  http://${API_HOST}:${API_PORT}/tokenmm"
  echo "  SocketIO: http://${API_HOST}:${API_PORT}/socket.io/?EIO=4&transport=polling"
}

health_stack() {
  load_env_file
  require_cmd curl
  echo "[tokenmm-stack] API health"
  curl -fsS "http://${API_HOST}:${API_PORT}/api/v1/healthz" | sed -n '1,4p'
  echo
  echo "[tokenmm-stack] TokenMM (GET)"
  local tokenmm_status
  tokenmm_status="$(curl -fsS -o /dev/null -w "%{http_code}" "http://${API_HOST}:${API_PORT}/tokenmm")"
  echo "HTTP ${tokenmm_status}"
  echo
  echo "[tokenmm-stack] Socket.IO polling handshake"
  local handshake
  handshake="$(curl -fsS "http://${API_HOST}:${API_PORT}/socket.io/?EIO=4&transport=polling")"
  echo "${handshake}" | sed -n '1,2p'
  if [[ "${handshake}" != *"sid"* ]]; then
    echo "[tokenmm-stack] socket handshake missing sid field" >&2
    exit 1
  fi
}

logs_stack() {
  local svc="${1:-}"
  if [[ -z "${svc}" ]]; then
    echo "[tokenmm-stack] choose service: redis|bridge|api|node_<strategy_slug>" >&2
    exit 1
  fi
  tail -n 120 -f "$(log_file "${svc}")"
}

main() {
  local cmd="${1:-}"
  case "${cmd}" in
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
      shift || true
      logs_stack "${1:-}"
      ;;
    -h | --help | help | "")
      usage
      ;;
    *)
      echo "[tokenmm-stack] unknown command: ${cmd}" >&2
      usage
      exit 1
      ;;
  esac
}

main "$@"
