#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
RUN_DIR="${RUN_DIR:-${ROOT_DIR}/.run/makerv3-prod}"
LOG_DIR="${RUN_DIR}/logs"
PID_DIR="${RUN_DIR}/pids"

CONFIG_PATH="${MAKERV3_CONFIG_PATH:-${ROOT_DIR}/examples/live/makerv3/config/makerv3.live.toml}"
ENV_PATH="${MAKERV3_ENV_PATH:-${ROOT_DIR}/examples/live/makerv3/config/makerv3.live.env}"
MODE="${MAKERV3_MODE:-live}"
CONFIRM_LIVE="${MAKERV3_CONFIRM_LIVE:-0}"
ENABLE_EXECUTION="${MAKERV3_ENABLE_EXECUTION:-0}"
ALLOW_MISSING_KEYS="${MAKERV3_ALLOW_MISSING_KEYS:-0}"
MANAGE_REDIS="${MAKERV3_MANAGE_REDIS:-1}"
REDIS_HOST="127.0.0.1"
REDIS_PORT="6380"
API_HOST="${MAKERV3_API_HOST:-127.0.0.1}"
API_PORT="${MAKERV3_API_PORT:-5022}"
START_TIMEOUT_SECS="${MAKERV3_START_TIMEOUT_SECS:-60}"
SKIP_FLUXBOARD_BUILD="${MAKERV3_SKIP_FLUXBOARD_BUILD:-0}"
PYTHON_BIN="${MAKERV3_PYTHON_BIN:-}"
LOAD_AWS_SECRETS="${MAKERV3_LOAD_AWS_SECRETS:-0}"
AWS_REGION="${MAKERV3_AWS_REGION:-ap-southeast-1}"
BYBIT_SECRET_ID="${MAKERV3_BYBIT_SECRET_ID:-/nautilus/makerv3/bybit}"
BINANCE_SECRET_ID="${MAKERV3_BINANCE_SECRET_ID:-/nautilus/makerv3/binance}"

readonly ROOT_DIR RUN_DIR LOG_DIR PID_DIR

usage() {
  cat << USAGE
Usage: scripts/deploy/makerv3_stack.sh <command>

Commands:
  start      Build and start redis (optional), node, bridge, and API + Fluxboard
  stop       Stop API, bridge, node, and managed redis process
  restart    Stop then start
  status     Show process and endpoint status
  health     Run health checks for API, tokenmm route, and socket handshake
  logs <svc> Tail service log (svc: redis|node|bridge|api)

Environment overrides:
  MAKERV3_ENV_PATH, MAKERV3_CONFIG_PATH, MAKERV3_MODE, MAKERV3_CONFIRM_LIVE
  MAKERV3_ENABLE_EXECUTION, MAKERV3_ALLOW_MISSING_KEYS, MAKERV3_MANAGE_REDIS
  MAKERV3_API_HOST, MAKERV3_API_PORT
  MAKERV3_SKIP_FLUXBOARD_BUILD, MAKERV3_START_TIMEOUT_SECS
USAGE
}

require_cmd() {
  local cmd="$1"
  if ! command -v "$cmd" > /dev/null 2>&1; then
    echo "[makerv3-stack] required command not found: ${cmd}" >&2
    exit 1
  fi
}

is_allowed_env_key() {
  local key="$1"
  case "${key}" in
    MAKERV3_* | BYBIT_* | BINANCE_*)
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
  echo "[makerv3-stack] required command not found: python3/python" >&2
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
          echo "[makerv3-stack] invalid key in env file ${ENV_PATH}: ${key}" >&2
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
        echo "[makerv3-stack] invalid line in env file ${ENV_PATH}: ${line}" >&2
        exit 1
      fi
    done < "${ENV_PATH}"
  fi
  CONFIG_PATH="${MAKERV3_CONFIG_PATH:-${CONFIG_PATH}}"
  MODE="${MAKERV3_MODE:-${MODE}}"
  CONFIRM_LIVE="${MAKERV3_CONFIRM_LIVE:-${CONFIRM_LIVE}}"
  ENABLE_EXECUTION="${MAKERV3_ENABLE_EXECUTION:-${ENABLE_EXECUTION}}"
  ALLOW_MISSING_KEYS="${MAKERV3_ALLOW_MISSING_KEYS:-${ALLOW_MISSING_KEYS}}"
  MANAGE_REDIS="${MAKERV3_MANAGE_REDIS:-${MANAGE_REDIS}}"
  API_HOST="${MAKERV3_API_HOST:-${API_HOST}}"
  API_PORT="${MAKERV3_API_PORT:-${API_PORT}}"
  START_TIMEOUT_SECS="${MAKERV3_START_TIMEOUT_SECS:-${START_TIMEOUT_SECS}}"
  SKIP_FLUXBOARD_BUILD="${MAKERV3_SKIP_FLUXBOARD_BUILD:-${SKIP_FLUXBOARD_BUILD}}"
  LOAD_AWS_SECRETS="${MAKERV3_LOAD_AWS_SECRETS:-${LOAD_AWS_SECRETS}}"
  AWS_REGION="${MAKERV3_AWS_REGION:-${AWS_REGION}}"
  BYBIT_SECRET_ID="${MAKERV3_BYBIT_SECRET_ID:-${BYBIT_SECRET_ID}}"
  BINANCE_SECRET_ID="${MAKERV3_BINANCE_SECRET_ID:-${BINANCE_SECRET_ID}}"
}

resolve_redis_target_from_config() {
  local pybin="$1"
  if [[ ! -f "${CONFIG_PATH}" ]]; then
    echo "[makerv3-stack] config not found: ${CONFIG_PATH}" >&2
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

load_secret_into_env() {
  local secret_id="$1"
  if [[ -z "${secret_id}" ]]; then
    return
  fi
  local raw
  if ! raw="$(aws secretsmanager get-secret-value --region "${AWS_REGION}" --secret-id "${secret_id}" --query SecretString --output text 2> /dev/null)"; then
    echo "[makerv3-stack] warning: failed to load secret ${secret_id}" >&2
    return
  fi
  if [[ -z "${raw}" || "${raw}" == "None" ]]; then
    echo "[makerv3-stack] warning: secret ${secret_id} is empty" >&2
    return
  fi

  while IFS='=' read -r key value; do
    [[ -z "${key}" ]] && continue
    if [[ "${key}" =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]]; then
      printf -v "${key}" "%s" "${value}"
      export "${key?}"
    fi
  done < <(printf '%s' "${raw}" | jq -r 'to_entries[] | "\(.key)=\(.value|tostring)"')
}

load_aws_secrets_if_enabled() {
  if [[ "${LOAD_AWS_SECRETS}" != "1" ]]; then
    return
  fi
  require_cmd aws
  require_cmd jq
  load_secret_into_env "${BYBIT_SECRET_ID}"
  load_secret_into_env "${BINANCE_SECRET_ID}"
}

validate_mode() {
  if [[ "${MODE}" != "paper" && "${MODE}" != "testnet" && "${MODE}" != "live" ]]; then
    echo "[makerv3-stack] invalid MAKERV3_MODE=${MODE}; expected paper|testnet|live" >&2
    exit 1
  fi
  if [[ "${MODE}" == "live" && "${CONFIRM_LIVE}" != "1" ]]; then
    echo "[makerv3-stack] refusing live startup: set MAKERV3_CONFIRM_LIVE=1" >&2
    exit 1
  fi
}

validate_config_and_keys() {
  if [[ ! -f "${CONFIG_PATH}" ]]; then
    echo "[makerv3-stack] config not found: ${CONFIG_PATH}" >&2
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
    echo "[makerv3-stack] missing required live credentials: ${missing[*]}" >&2
    echo "[makerv3-stack] set MAKERV3_ALLOW_MISSING_KEYS=1 only for market-data smoke." >&2
    exit 1
  fi
}

ensure_dirs() {
  mkdir -p "${LOG_DIR}" "${PID_DIR}"
}

pid_file() {
  local svc="$1"
  echo "${PID_DIR}/${svc}.pid"
}

log_file() {
  local svc="$1"
  echo "${LOG_DIR}/${svc}.log"
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
  if [[ -z "${pid}" ]]; then
    return 1
  fi
  if kill -0 "${pid}" > /dev/null 2>&1; then
    return 0
  fi
  return 1
}

start_process() {
  local svc="$1"
  shift
  local file
  file="$(pid_file "${svc}")"
  local log
  log="$(log_file "${svc}")"

  if is_running "${svc}"; then
    echo "[makerv3-stack] ${svc} already running (pid $(cat "${file}"))"
    return
  fi

  echo "[makerv3-stack] starting ${svc}"
  rm -f "${file}"
  if (($# == 0)); then
    echo "[makerv3-stack] refusing to start ${svc}: missing command" >&2
    exit 1
  fi
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
  if [[ ! -f "${file}" ]]; then
    echo "[makerv3-stack] ${svc} failed to write pid file" >&2
    tail -n 80 "${log}" >&2 || true
    exit 1
  fi
  local pid
  pid="$(cat "${file}")"

  if ! kill -0 "${pid}" > /dev/null 2>&1; then
    echo "[makerv3-stack] ${svc} failed to start; log tail:" >&2
    tail -n 80 "${log}" >&2 || true
    exit 1
  fi
}

stop_process() {
  local svc="$1"
  local file
  file="$(pid_file "${svc}")"

  if [[ ! -f "${file}" ]]; then
    return
  fi

  local pid
  pid="$(cat "${file}")"
  if [[ -n "${pid}" ]] && kill -0 "${pid}" > /dev/null 2>&1; then
    echo "[makerv3-stack] stopping ${svc} (pid ${pid})"
    kill "${pid}" > /dev/null 2>&1 || true
    for _ in {1..20}; do
      if ! kill -0 "${pid}" > /dev/null 2>&1; then
        break
      fi
      sleep 0.5
    done
    if kill -0 "${pid}" > /dev/null 2>&1; then
      echo "[makerv3-stack] force-killing ${svc} (pid ${pid})"
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
      echo "[makerv3-stack] ${label} ready: ${url}"
      return
    fi
    sleep 1
  done

  echo "[makerv3-stack] timeout waiting for ${label}: ${url}" >&2
  exit 1
}

start_redis_if_needed() {
  require_cmd redis-cli
  if redis-cli -h "${REDIS_HOST}" -p "${REDIS_PORT}" ping > /dev/null 2>&1; then
    echo "[makerv3-stack] redis already reachable at ${REDIS_HOST}:${REDIS_PORT}"
    return
  fi

  if [[ "${MANAGE_REDIS}" != "1" ]]; then
    echo "[makerv3-stack] redis not reachable at ${REDIS_HOST}:${REDIS_PORT}" >&2
    echo "[makerv3-stack] start redis manually or set MAKERV3_MANAGE_REDIS=1" >&2
    exit 1
  fi

  if [[ "${REDIS_HOST}" != "127.0.0.1" && "${REDIS_HOST}" != "localhost" ]]; then
    echo "[makerv3-stack] refusing to manage redis for non-local host ${REDIS_HOST}:${REDIS_PORT}" >&2
    echo "[makerv3-stack] update [redis] host/port in ${CONFIG_PATH} to use a local redis instance" >&2
    exit 1
  fi

  require_cmd redis-server
  start_process "redis" redis-server --bind "${REDIS_HOST}" --port "${REDIS_PORT}" --save "" --appendonly no

  local deadline=$((SECONDS + START_TIMEOUT_SECS))
  while ((SECONDS < deadline)); do
    if redis-cli -h "${REDIS_HOST}" -p "${REDIS_PORT}" ping > /dev/null 2>&1; then
      echo "[makerv3-stack] managed redis started at ${REDIS_HOST}:${REDIS_PORT}"
      return
    fi
    sleep 1
  done

  echo "[makerv3-stack] managed redis failed to answer PING" >&2
  tail -n 80 "$(log_file redis)" >&2 || true
  exit 1
}

build_fluxboard() {
  if [[ "${SKIP_FLUXBOARD_BUILD}" == "1" ]]; then
    echo "[makerv3-stack] skipping fluxboard build (MAKERV3_SKIP_FLUXBOARD_BUILD=1)"
    return
  fi

  require_cmd pnpm
  if [[ ! -d "${ROOT_DIR}/fluxboard/node_modules" ]]; then
    echo "[makerv3-stack] installing fluxboard dependencies"
    pnpm --dir "${ROOT_DIR}/fluxboard" install --frozen-lockfile
  fi
  echo "[makerv3-stack] building fluxboard"
  pnpm --dir "${ROOT_DIR}/fluxboard" build
}

start_stack() {
  require_cmd curl
  local pybin
  pybin="$(resolve_python_bin)"

  load_env_file
  load_aws_secrets_if_enabled
  validate_mode
  validate_config_and_keys
  ensure_dirs

  resolve_redis_target_from_config "${pybin}"
  start_redis_if_needed
  build_fluxboard

  local live_flag=""
  if [[ "${MODE}" == "live" ]]; then
    live_flag="--confirm-live"
  fi

  local exec_flag=""
  if [[ "${ENABLE_EXECUTION}" == "1" ]]; then
    exec_flag="--enable-execution"
  fi

  local -a node_cmd=(env "PYTHONPATH=${ROOT_DIR}:${PYTHONPATH:-}" "${pybin}" examples/live/makerv3/run_node.py --config "${CONFIG_PATH}" --mode "${MODE}")
  if [[ -n "${live_flag}" ]]; then
    node_cmd+=("${live_flag}")
  fi
  if [[ -n "${exec_flag}" ]]; then
    node_cmd+=("${exec_flag}")
  fi
  start_process "node" "${node_cmd[@]}"

  local -a bridge_cmd=(env "PYTHONPATH=${ROOT_DIR}:${PYTHONPATH:-}" "${pybin}" examples/live/makerv3/run_bridge.py --config "${CONFIG_PATH}" --mode "${MODE}")
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

  echo "[makerv3-stack] stack started"
  status_stack
}

stop_stack() {
  stop_process "api"
  stop_process "bridge"
  stop_process "node"
  stop_process "redis"
  echo "[makerv3-stack] stack stopped"
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
  printf '%-8s %-10s pid=%s log=%s\n' "${svc}" "${state}" "${pid}" "$(log_file "${svc}")"
}

status_stack() {
  echo "[makerv3-stack] status"
  service_status_line "redis"
  service_status_line "node"
  service_status_line "bridge"
  service_status_line "api"
  echo "[makerv3-stack] endpoints"
  echo "  API:      http://${API_HOST}:${API_PORT}/api/v1/healthz"
  echo "  TokenMM:  http://${API_HOST}:${API_PORT}/tokenmm"
  echo "  SocketIO: http://${API_HOST}:${API_PORT}/socket.io/?EIO=4&transport=polling"
}

health_stack() {
  require_cmd curl
  echo "[makerv3-stack] API health"
  curl -fsS "http://${API_HOST}:${API_PORT}/api/v1/healthz" | sed -n '1,3p'
  echo
  echo "[makerv3-stack] TokenMM"
  curl -fsSI "http://${API_HOST}:${API_PORT}/tokenmm" | sed -n '1,5p'
  echo
  echo "[makerv3-stack] Socket.IO polling handshake"
  curl -fsS "http://${API_HOST}:${API_PORT}/socket.io/?EIO=4&transport=polling" | sed -n '1,2p'
}

logs_stack() {
  local svc="${1:-}"
  if [[ -z "${svc}" ]]; then
    echo "[makerv3-stack] choose service: redis|node|bridge|api" >&2
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
      echo "[makerv3-stack] unknown command: ${cmd}" >&2
      usage
      exit 1
      ;;
  esac
}

main "$@"
