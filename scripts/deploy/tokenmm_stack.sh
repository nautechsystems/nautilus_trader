#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
RUN_DIR="${RUN_DIR:-${ROOT_DIR}/.run/tokenmm-stack}"
LOG_DIR="${RUN_DIR}/logs"
PID_DIR="${RUN_DIR}/pids"
DEFAULT_ENV_PATH="${ROOT_DIR}/deploy/tokenmm/tokenmm_stack.env"

CONFIG_PATH="${TOKENMM_CONFIG_PATH:-${ROOT_DIR}/deploy/tokenmm/tokenmm.live.toml}"
STRATEGIES_DIR="${TOKENMM_STRATEGIES_DIR:-${ROOT_DIR}/deploy/tokenmm/strategies}"
ENV_PATH="${TOKENMM_ENV_PATH:-${DEFAULT_ENV_PATH}}"
MODE="${TOKENMM_MODE:-paper}"
CONFIRM_LIVE="${TOKENMM_CONFIRM_LIVE:-0}"
ENABLE_EXECUTION="${TOKENMM_ENABLE_EXECUTION:-0}"
ALLOW_MISSING_KEYS="${TOKENMM_ALLOW_MISSING_KEYS:-0}"
MANAGE_REDIS="${TOKENMM_MANAGE_REDIS:-1}"
API_HOST="${TOKENMM_API_HOST:-}"
API_PORT="${TOKENMM_API_PORT:-}"
EXPECTED_NODES="${TOKENMM_EXPECTED_NODES:-0}"
START_TIMEOUT_SECS="${TOKENMM_START_TIMEOUT_SECS:-90}"
BALANCES_READY_TIMEOUT_SECS="${TOKENMM_BALANCES_READY_TIMEOUT_SECS:-90}"
SKIP_FLUXBOARD_BUILD="${TOKENMM_SKIP_FLUXBOARD_BUILD:-0}"
STRICT_PROFILE_CHECK="${TOKENMM_STRICT_PROFILE_CHECK:-1}"
STRICT_BALANCES_READY_CHECK="${TOKENMM_STRICT_BALANCES_READY_CHECK:-1}"
PYTHON_BIN="${TOKENMM_PYTHON_BIN:-}"
LOAD_AWS_SECRETS="${TOKENMM_LOAD_AWS_SECRETS:-0}"
AWS_REGION="${TOKENMM_AWS_REGION:-ap-southeast-1}"
BYBIT_SECRET_ID="${TOKENMM_BYBIT_SECRET_ID:-/nautilus/tokenmm/bybit}"
BINANCE_SECRET_ID="${TOKENMM_BINANCE_SECRET_ID:-/nautilus/tokenmm/binance}"
OKX_SECRET_ID="${TOKENMM_OKX_SECRET_ID:-/nautilus/tokenmm/okx}"

REDIS_HOST="127.0.0.1"
REDIS_PORT="6380"
REDIS_DB="0"
REDIS_USERNAME=""
REDIS_PASSWORD=""
REDIS_SSL="0"

declare -a STRATEGY_CONFIGS=()
readonly ROOT_DIR RUN_DIR LOG_DIR PID_DIR
STARTUP_CLEANUP_ON_EXIT=0

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
  TOKENMM_BALANCES_READY_TIMEOUT_SECS
  TOKENMM_STRICT_PROFILE_CHECK, TOKENMM_STRICT_BALANCES_READY_CHECK
  TOKENMM_LOAD_AWS_SECRETS, TOKENMM_AWS_REGION
  TOKENMM_BYBIT_SECRET_ID, TOKENMM_BINANCE_SECRET_ID, TOKENMM_OKX_SECRET_ID
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
    TOKENMM_* | BYBIT_* | BINANCE_* | OKX_*)
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
  if [[ -n "${TOKENMM_ENV_PATH:-}" && ! -f "${ENV_PATH}" ]]; then
    echo "[tokenmm-stack] env file not found: ${ENV_PATH}" >&2
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
  BALANCES_READY_TIMEOUT_SECS="${TOKENMM_BALANCES_READY_TIMEOUT_SECS:-${BALANCES_READY_TIMEOUT_SECS}}"
  SKIP_FLUXBOARD_BUILD="${TOKENMM_SKIP_FLUXBOARD_BUILD:-${SKIP_FLUXBOARD_BUILD}}"
  STRICT_PROFILE_CHECK="${TOKENMM_STRICT_PROFILE_CHECK:-${STRICT_PROFILE_CHECK}}"
  STRICT_BALANCES_READY_CHECK="${TOKENMM_STRICT_BALANCES_READY_CHECK:-${STRICT_BALANCES_READY_CHECK}}"
  LOAD_AWS_SECRETS="${TOKENMM_LOAD_AWS_SECRETS:-${LOAD_AWS_SECRETS}}"
  AWS_REGION="${TOKENMM_AWS_REGION:-${AWS_REGION}}"
  BYBIT_SECRET_ID="${TOKENMM_BYBIT_SECRET_ID:-${BYBIT_SECRET_ID}}"
  BINANCE_SECRET_ID="${TOKENMM_BINANCE_SECRET_ID:-${BINANCE_SECRET_ID}}"
  OKX_SECRET_ID="${TOKENMM_OKX_SECRET_ID:-${OKX_SECRET_ID}}"
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
import os

path = Path(sys.argv[1])
data = tomllib.load(path.open("rb"))
redis_cfg = data.get("redis") or {}
host = str(os.getenv("TOKENMM_REDIS_HOST") or redis_cfg.get("host", "127.0.0.1")).strip() or "127.0.0.1"
port = int(os.getenv("TOKENMM_REDIS_PORT") or redis_cfg.get("port", 6380))
db = int(os.getenv("TOKENMM_REDIS_DB") or redis_cfg.get("db", 0))
username = str(os.getenv("TOKENMM_REDIS_USERNAME") or redis_cfg.get("username", "")).strip()
password = str(os.getenv("TOKENMM_REDIS_PASSWORD") or redis_cfg.get("password", "")).strip()
ssl_raw = str(os.getenv("TOKENMM_REDIS_SSL") or redis_cfg.get("ssl", False)).strip().lower()
ssl = "1" if ssl_raw in {"1", "true", "yes", "on"} else "0"
print(host)
print(port)
print(db)
print(username)
print(password)
print(ssl)
PY
  )"
  mapfile -t redis_runtime <<< "${output}"
  REDIS_HOST="${redis_runtime[0]}"
  REDIS_PORT="${redis_runtime[1]}"
  REDIS_DB="${redis_runtime[2]}"
  REDIS_USERNAME="${redis_runtime[3]}"
  REDIS_PASSWORD="${redis_runtime[4]}"
  REDIS_SSL="${redis_runtime[5]}"
}

resolve_api_bind_from_config() {
  local pybin="$1"
  if [[ ! -f "${CONFIG_PATH}" ]]; then
    echo "[tokenmm-stack] config not found: ${CONFIG_PATH}" >&2
    exit 1
  fi

  local output
  output="$(
    "${pybin}" - "${CONFIG_PATH}" "${API_HOST}" "${API_PORT}" <<'PY'
import sys
import tomllib
from pathlib import Path

config_path = Path(sys.argv[1])
host_override = sys.argv[2].strip()
port_override = sys.argv[3].strip()

data = tomllib.load(config_path.open("rb"))
api_cfg = data.get("api") or {}
if not isinstance(api_cfg, dict):
    raise SystemExit("invalid [api] config")

host = host_override or str(api_cfg.get("host", "127.0.0.1")).strip() or "127.0.0.1"
port = port_override or str(api_cfg.get("port", 5022)).strip() or "5022"
print(host)
print(port)
PY
  )"
  mapfile -t api_runtime <<< "${output}"
  API_HOST="${api_runtime[0]}"
  API_PORT="${api_runtime[1]}"
}

load_secret_into_env() {
  local secret_id="$1"
  if [[ -z "${secret_id}" ]]; then
    return
  fi
  local raw
  if ! raw="$(aws secretsmanager get-secret-value --region "${AWS_REGION}" --secret-id "${secret_id}" --query SecretString --output text 2> /dev/null)"; then
    echo "[tokenmm-stack] warning: failed to load secret ${secret_id}" >&2
    return
  fi
  if [[ -z "${raw}" || "${raw}" == "None" ]]; then
    echo "[tokenmm-stack] warning: secret ${secret_id} is empty" >&2
    return
  fi

  while IFS='=' read -r key value; do
    [[ -z "${key}" ]] && continue
    if [[ ! "${key}" =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]]; then
      continue
    fi
    case "${key}" in
      BYBIT_*|BINANCE_*|OKX_*)
        printf -v "${key}" "%s" "${value}"
        export "${key?}"
        ;;
      *)
        echo "[tokenmm-stack] warning: skipping unsupported secret key ${key}" >&2
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
  load_secret_into_env "${BYBIT_SECRET_ID}"
  load_secret_into_env "${BINANCE_SECRET_ID}"
  load_secret_into_env "${OKX_SECRET_ID}"
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

api_request_host() {
  if [[ "${API_HOST}" == "0.0.0.0" ]]; then
    echo "127.0.0.1"
    return
  fi
  if [[ "${API_HOST}" == "::" ]]; then
    echo "::1"
    return
  fi
  echo "${API_HOST}"
}

api_request_base_url() {
  local request_host
  request_host="$(api_request_host)"
  echo "http://${request_host}:${API_PORT}"
}

log_runtime_intent() {
  echo "[tokenmm-stack] runtime intent: mode=${MODE} confirm_live=${CONFIRM_LIVE} enable_execution=${ENABLE_EXECUTION}"
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
  [[ -z "${OKX_API_KEY:-}" ]] && missing+=("OKX_API_KEY")
  [[ -z "${OKX_API_SECRET:-}" ]] && missing+=("OKX_API_SECRET")
  [[ -z "${OKX_API_PASSPHRASE:-}" ]] && missing+=("OKX_API_PASSPHRASE")
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

read_tokenmm_registry_ids() {
  local pybin="$1"
  local config_path="$2"
  "${pybin}" - "${config_path}" << 'PY'
import sys
from pathlib import Path
import tomllib

data = tomllib.load(Path(sys.argv[1]).open("rb"))
api_cfg = data.get("api") or {}
raw_ids = api_cfg.get("tokenmm_strategy_ids") or []
if isinstance(raw_ids, str):
    raw_ids = [raw_ids]

seen: set[str] = set()
if isinstance(raw_ids, list | tuple):
    for item in raw_ids:
        text = str(item).strip()
        if not text or text in seen:
            continue
        seen.add(text)
        print(text)
PY
}

validate_strategy_registry_alignment() {
  local pybin="$1"
  declare -A seen_deployed_ids=()
  declare -A seen_nautilus_strategy_ids=()
  local -a deployed_ids=()
  local -a registry_ids=()
  local -a missing_from_configs=()
  local -a unexpected_in_configs=()

  for config_path in "${STRATEGY_CONFIGS[@]}"; do
    local strategy_id
    local nautilus_strategy_id
    strategy_id="$(strategy_flux_id "${pybin}" "${config_path}")"
    nautilus_strategy_id="$(strategy_nautilus_id "${pybin}" "${config_path}")"
    if [[ -n "${seen_deployed_ids[${strategy_id}]:-}" ]]; then
      echo "[tokenmm-stack] duplicate [identity].strategy_id in strategy configs: ${strategy_id}" >&2
      exit 1
    fi
    if [[ -n "${seen_nautilus_strategy_ids[${nautilus_strategy_id}]:-}" ]]; then
      echo "[tokenmm-stack] duplicate [strategy].strategy_id in strategy configs: ${nautilus_strategy_id}" >&2
      exit 1
    fi
    seen_deployed_ids["${strategy_id}"]=1
    seen_nautilus_strategy_ids["${nautilus_strategy_id}"]=1
    deployed_ids+=("${strategy_id}")
  done

  mapfile -t registry_ids < <(read_tokenmm_registry_ids "${pybin}" "${CONFIG_PATH}")
  if ((${#registry_ids[@]} == 0)); then
    echo "[tokenmm-stack] [api].tokenmm_strategy_ids must be non-empty" >&2
    exit 1
  fi

  declare -A registry_map=()
  for strategy_id in "${registry_ids[@]}"; do
    registry_map["${strategy_id}"]=1
    if [[ -z "${seen_deployed_ids[${strategy_id}]:-}" ]]; then
      missing_from_configs+=("${strategy_id}")
    fi
  done

  for strategy_id in "${deployed_ids[@]}"; do
    if [[ -z "${registry_map[${strategy_id}]:-}" ]]; then
      unexpected_in_configs+=("${strategy_id}")
    fi
  done

  if ((${#missing_from_configs[@]} > 0 || ${#unexpected_in_configs[@]} > 0)); then
    echo "[tokenmm-stack] TokenMM registry mismatch between [api].tokenmm_strategy_ids and strategies.d configs" >&2
    if ((${#missing_from_configs[@]} > 0)); then
      echo "[tokenmm-stack] missing strategy configs for registry IDs: ${missing_from_configs[*]}" >&2
    fi
    if ((${#unexpected_in_configs[@]} > 0)); then
      echo "[tokenmm-stack] strategies.d IDs not present in registry allowlist: ${unexpected_in_configs[*]}" >&2
    fi
    exit 1
  fi
}

assert_tokenmm_profile_params_alignment() {
  local pybin="$1"
  if [[ "${STRICT_PROFILE_CHECK}" != "1" ]]; then
    echo "[tokenmm-stack] skipping TokenMM profile assertions (TOKENMM_STRICT_PROFILE_CHECK=${STRICT_PROFILE_CHECK})"
    return
  fi

  local -a expected_ids=()
  mapfile -t expected_ids < <(read_tokenmm_registry_ids "${pybin}" "${CONFIG_PATH}")
  if ((${#expected_ids[@]} == 0)); then
    echo "[tokenmm-stack] [api].tokenmm_strategy_ids must be non-empty" >&2
    exit 1
  fi

  local request_host
  request_host="$(api_request_host)"

  local response
  response="$(curl -fsS "http://${request_host}:${API_PORT}/api/v1/params?profile=tokenmm")"

  local expected_joined
  expected_joined="$(printf '%s\n' "${expected_ids[@]}")"
  EXPECTED_IDS_NL="${expected_joined}" \
  EXPECTED_NODES="${EXPECTED_NODES}" \
  "${pybin}" - "${response}" << 'PY'
import json
import os
import sys

payload = json.loads(sys.argv[1])
items = payload.get("data")
if not isinstance(items, list):
    raise SystemExit("[tokenmm-stack] /api/v1/params payload missing list `data`")

actual: list[str] = []
seen: set[str] = set()
for item in items:
    if not isinstance(item, dict):
        continue
    strategy_id = str(item.get("strategy_id", "")).strip()
    if not strategy_id or strategy_id in seen:
        continue
    seen.add(strategy_id)
    actual.append(strategy_id)

expected = [
    line.strip()
    for line in os.environ.get("EXPECTED_IDS_NL", "").splitlines()
    if line.strip()
]
expected_count = int(os.environ.get("EXPECTED_NODES", "0") or "0")
if expected_count <= 0:
    expected_count = len(expected)

actual_set = set(actual)
expected_set = set(expected)
if len(actual) != expected_count:
    raise SystemExit(
        f"[tokenmm-stack] /api/v1/params?profile=tokenmm returned {len(actual)} strategy IDs, expected {expected_count}: actual={actual}"
    )
if actual_set != expected_set:
    missing = sorted(expected_set - actual_set)
    unexpected = sorted(actual_set - expected_set)
    raise SystemExit(
        f"[tokenmm-stack] /api/v1/params?profile=tokenmm IDs mismatch: missing={missing} unexpected={unexpected} actual={actual}"
    )
PY

  echo "[tokenmm-stack] TokenMM profile check passed: ${#expected_ids[@]} strategy IDs visible in /api/v1/params"
}

assert_tokenmm_profile_balances_readiness() {
  local pybin="$1"
  if [[ "${STRICT_BALANCES_READY_CHECK}" != "1" ]]; then
    echo "[tokenmm-stack] skipping TokenMM balances readiness assertions (TOKENMM_STRICT_BALANCES_READY_CHECK=${STRICT_BALANCES_READY_CHECK})"
    return
  fi

  local deadline=$((SECONDS + BALANCES_READY_TIMEOUT_SECS))
  local last_error="waiting_for_balances"
  local request_host
  request_host="$(api_request_host)"
  while ((SECONDS < deadline)); do
    local response
    if ! response="$(curl -fsS "http://${request_host}:${API_PORT}/api/v1/balances?profile=tokenmm" 2> /dev/null)"; then
      last_error="request_failed"
      sleep 1
      continue
    fi

    local detail
    if detail="$(
      "${pybin}" - "${response}" << 'PY'
import json
import sys

payload = json.loads(sys.argv[1])
data = payload.get("data")
if not isinstance(data, dict):
    raise SystemExit("missing `data` object")

missing_required = data.get("missing_required") or []
if not isinstance(missing_required, list):
    raise SystemExit("`missing_required` is not a list")

missing_required = sorted(str(item).strip() for item in missing_required if str(item).strip())
if missing_required:
    raise SystemExit(f"missing_required={missing_required}")

components = data.get("components")
if not isinstance(components, list) or not components:
    raise SystemExit("missing `components` list")

required_ids: list[str] = []
required_missing: list[str] = []
required_stale: list[str] = []
for component in components:
    if not isinstance(component, dict):
        continue
    strategy_id = str(component.get("strategy_id", "")).strip()
    if not strategy_id or not bool(component.get("required")):
        continue
    required_ids.append(strategy_id)
    if bool(component.get("missing")):
        required_missing.append(strategy_id)
    if not bool(component.get("stale")):
        continue
    required_stale.append(strategy_id)

if not required_ids:
    raise SystemExit("no required components in balances payload")
if required_missing:
    raise SystemExit(f"required_missing={sorted(set(required_missing))}")
if required_stale:
    raise SystemExit(f"required_stale={sorted(set(required_stale))}")

print(f"required_ready={len(required_ids)}")
PY
    )"; then
      echo "[tokenmm-stack] TokenMM balances readiness check passed (${detail})"
      return
    fi

    last_error="${detail}"
    sleep 1
  done

  echo "[tokenmm-stack] timeout waiting for TokenMM balances readiness after ${BALANCES_READY_TIMEOUT_SECS}s: ${last_error}" >&2
  exit 1
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

strategy_nautilus_id() {
  local pybin="$1"
  local config_path="$2"
  "${pybin}" - "${config_path}" << 'PY'
import sys
from pathlib import Path
import tomllib

data = tomllib.load(Path(sys.argv[1]).open("rb"))
strategy = data.get("strategy") or {}
strategy_id = str(strategy.get("strategy_id", "")).strip()
if not strategy_id:
    raise SystemExit("missing [strategy].strategy_id")
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
    if command -v setsid > /dev/null 2>&1; then
      if command -v nohup > /dev/null 2>&1; then
        setsid nohup "$@" >> "${log}" 2>&1 < /dev/null &
      else
        setsid "$@" >> "${log}" 2>&1 < /dev/null &
      fi
    elif command -v nohup > /dev/null 2>&1; then
      nohup "$@" >> "${log}" 2>&1 < /dev/null &
    else
      "$@" >> "${log}" 2>&1 < /dev/null &
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

can_ping_redis_target() {
  local pybin="$1"
  REDIS_HOST="${REDIS_HOST}" \
  REDIS_PORT="${REDIS_PORT}" \
  REDIS_DB="${REDIS_DB}" \
  REDIS_USERNAME="${REDIS_USERNAME}" \
  REDIS_PASSWORD="${REDIS_PASSWORD}" \
  REDIS_SSL="${REDIS_SSL}" \
  "${pybin}" - << 'PY'
import os
import sys

import redis

client = redis.Redis(
    host=os.environ["REDIS_HOST"],
    port=int(os.environ["REDIS_PORT"]),
    db=int(os.environ.get("REDIS_DB", "0") or "0"),
    username=(os.environ.get("REDIS_USERNAME") or None),
    password=(os.environ.get("REDIS_PASSWORD") or None),
    ssl=os.environ.get("REDIS_SSL", "0") == "1",
    socket_connect_timeout=3.0,
    socket_timeout=3.0,
    decode_responses=True,
)
try:
    ok = client.ping()
except Exception:
    raise SystemExit(1)
raise SystemExit(0 if ok else 1)
PY
}

start_redis_if_needed() {
  local pybin="$1"
  if can_ping_redis_target "${pybin}"; then
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
    local -a cmd=(
      env
      "PYTHONPATH=${ROOT_DIR}:${PYTHONPATH:-}"
      "TOKENMM_REDIS_HOST=${REDIS_HOST}"
      "TOKENMM_REDIS_PORT=${REDIS_PORT}"
      "TOKENMM_REDIS_DB=${REDIS_DB}"
      "TOKENMM_REDIS_USERNAME=${REDIS_USERNAME}"
      "TOKENMM_REDIS_PASSWORD=${REDIS_PASSWORD}"
      "TOKENMM_REDIS_SSL=${REDIS_SSL}"
      "${pybin}"
      -m
      nautilus_trader.flux.runners.tokenmm.run_node
      --config
      "${config_path}"
      --shared-config
      "${CONFIG_PATH}"
      --mode
      "${MODE}"
    )
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
  resolve_api_bind_from_config "${pybin}"
  load_aws_secrets_if_enabled
  validate_mode
  log_runtime_intent
  validate_config_and_keys
  load_strategy_configs
  validate_strategy_registry_alignment "${pybin}"
  ensure_dirs

  resolve_redis_target_from_config "${pybin}"
  start_redis_if_needed "${pybin}"
  build_fluxboard
  start_nodes "${pybin}"

  local live_flag=""
  if [[ "${MODE}" == "live" ]]; then
    live_flag="--confirm-live"
  fi

  local -a bridge_cmd=(
    env
    "PYTHONPATH=${ROOT_DIR}:${PYTHONPATH:-}"
    "TOKENMM_REDIS_HOST=${REDIS_HOST}"
    "TOKENMM_REDIS_PORT=${REDIS_PORT}"
    "TOKENMM_REDIS_DB=${REDIS_DB}"
    "TOKENMM_REDIS_USERNAME=${REDIS_USERNAME}"
    "TOKENMM_REDIS_PASSWORD=${REDIS_PASSWORD}"
    "TOKENMM_REDIS_SSL=${REDIS_SSL}"
    "${pybin}"
    -m
    nautilus_trader.flux.runners.tokenmm.run_bridge
    --config
    "${CONFIG_PATH}"
    --mode
    "${MODE}"
    --all-strategies
  )
  if [[ -n "${live_flag}" ]]; then
    bridge_cmd+=("${live_flag}")
  fi
  start_process "bridge" "${bridge_cmd[@]}"

  local -a api_cmd=(
    env
    "PYTHONPATH=${ROOT_DIR}:${PYTHONPATH:-}"
    "TOKENMM_REDIS_HOST=${REDIS_HOST}"
    "TOKENMM_REDIS_PORT=${REDIS_PORT}"
    "TOKENMM_REDIS_DB=${REDIS_DB}"
    "TOKENMM_REDIS_USERNAME=${REDIS_USERNAME}"
    "TOKENMM_REDIS_PASSWORD=${REDIS_PASSWORD}"
    "TOKENMM_REDIS_SSL=${REDIS_SSL}"
    "${pybin}"
    -m
    nautilus_trader.flux.runners.tokenmm.run_api
    --config
    "${CONFIG_PATH}"
    --mode
    "${MODE}"
  )
  if [[ -n "${live_flag}" ]]; then
    api_cmd+=("${live_flag}")
  fi
  api_cmd+=(--host "${API_HOST}" --port "${API_PORT}" --serve-fluxboard)
  start_process "api" "${api_cmd[@]}"

  local request_base_url
  request_base_url="$(api_request_base_url)"

  wait_for_url "${request_base_url}/api/v1/healthz" "flux api"
  wait_for_url "${request_base_url}/tokenmm" "fluxboard tokenmm"
  assert_tokenmm_profile_params_alignment "${pybin}"
  assert_tokenmm_profile_balances_readiness "${pybin}"
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

cleanup_partial_startup_on_exit() {
  local exit_code=$?
  if [[ "${STARTUP_CLEANUP_ON_EXIT}" == "1" && "${exit_code}" != "0" ]]; then
    echo "[tokenmm-stack] startup failed; stopping partial stack" >&2
    STARTUP_CLEANUP_ON_EXIT=0
    stop_stack || true
  fi
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
  local pybin
  pybin="$(resolve_python_bin)"
  resolve_api_bind_from_config "${pybin}"
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
  local pybin
  pybin="$(resolve_python_bin)"
  resolve_api_bind_from_config "${pybin}"
  local request_base_url
  request_base_url="$(api_request_base_url)"
  echo "[tokenmm-stack] API health"
  curl -fsS "${request_base_url}/api/v1/healthz" | sed -n '1,4p'
  echo
  echo "[tokenmm-stack] TokenMM (GET)"
  local tokenmm_status
  tokenmm_status="$(curl -fsS -o /dev/null -w "%{http_code}" "${request_base_url}/tokenmm")"
  echo "HTTP ${tokenmm_status}"
  echo
  echo "[tokenmm-stack] Socket.IO polling handshake"
  local handshake
  handshake="$(curl -fsS "${request_base_url}/socket.io/?EIO=4&transport=polling")"
  echo "${handshake}" | sed -n '1,2p'
  if [[ "${handshake}" != *"sid"* ]]; then
    echo "[tokenmm-stack] socket handshake missing sid field" >&2
    exit 1
  fi
  echo
  echo "[tokenmm-stack] TokenMM params profile assertions"
  assert_tokenmm_profile_params_alignment "${pybin}"
  echo
  echo "[tokenmm-stack] TokenMM balances readiness assertions"
  assert_tokenmm_profile_balances_readiness "${pybin}"
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
      STARTUP_CLEANUP_ON_EXIT=1
      start_stack
      STARTUP_CLEANUP_ON_EXIT=0
      ;;
    stop)
      stop_stack
      ;;
    restart)
      stop_stack
      STARTUP_CLEANUP_ON_EXIT=1
      start_stack
      STARTUP_CLEANUP_ON_EXIT=0
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

trap cleanup_partial_startup_on_exit EXIT

main "$@"
