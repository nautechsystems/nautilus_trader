#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)"
WORKDIR="${WORKDIR:-${ROOT_DIR}}"
AWS_REGION="${TOKENMM_AWS_REGION:-ap-southeast-1}"
SECRET_ID="${NAUTILUS_TELEMETRY_PG_SECRET_ID:-}"
CONFIG_PATH=""
EXIT_CONFIG=78
PASSTHROUGH_ARGS=()

require_cmd() {
  local name="$1"
  if ! command -v "${name}" > /dev/null 2>&1; then
    echo "[tokenmm-telemetry-shipper] missing required command: ${name}" >&2
    exit "${EXIT_CONFIG}"
  fi
}

parse_args() {
  while (($#)); do
    case "$1" in
      --config)
        shift
        [[ $# -gt 0 ]] || {
          echo "[tokenmm-telemetry-shipper] missing value for --config" >&2
          exit "${EXIT_CONFIG}"
        }
        CONFIG_PATH="$1"
        ;;
      --once|--bootstrap-postgres)
        PASSTHROUGH_ARGS+=("$1")
        ;;
      *)
        PASSTHROUGH_ARGS+=("$1")
        ;;
    esac
    shift
  done
}

load_secret_value() {
  require_cmd aws
  require_cmd jq

  local raw=""
  if ! raw="$(aws secretsmanager get-secret-value --region "${AWS_REGION}" --secret-id "${SECRET_ID}" --query SecretString --output text 2> /dev/null)"; then
    echo "[tokenmm-telemetry-shipper] failed to load secret ${SECRET_ID}" >&2
    exit 78
  fi
  if [[ -z "${raw}" || "${raw}" == "None" ]]; then
    echo "[tokenmm-telemetry-shipper] secret ${SECRET_ID} is empty" >&2
    exit 78
  fi

  export NAUTILUS_TELEMETRY_PG_HOST="${NAUTILUS_TELEMETRY_PG_HOST:-$(printf '%s' "${raw}" | jq -r '.host // empty')}"
  export NAUTILUS_TELEMETRY_PG_PORT="${NAUTILUS_TELEMETRY_PG_PORT:-$(printf '%s' "${raw}" | jq -r '(.port // 5432)|tostring')}"
  export NAUTILUS_TELEMETRY_PG_DATABASE="${NAUTILUS_TELEMETRY_PG_DATABASE:-$(printf '%s' "${raw}" | jq -r '.database // empty')}"
  export NAUTILUS_TELEMETRY_PG_SCHEMA="${NAUTILUS_TELEMETRY_PG_SCHEMA:-$(printf '%s' "${raw}" | jq -r '.schema // "telemetry"')}"
  export NAUTILUS_TELEMETRY_PG_USERNAME="${NAUTILUS_TELEMETRY_PG_USERNAME:-$(printf '%s' "${raw}" | jq -r '.username // empty')}"
  export NAUTILUS_TELEMETRY_PG_PASSWORD="${NAUTILUS_TELEMETRY_PG_PASSWORD:-$(printf '%s' "${raw}" | jq -r '.password // empty')}"
  export NAUTILUS_TELEMETRY_PG_SSLMODE="${NAUTILUS_TELEMETRY_PG_SSLMODE:-$(printf '%s' "${raw}" | jq -r '.sslmode // "require"')}"
}

require_pg_env() {
  local missing=0
  local key=""
  for key in \
    NAUTILUS_TELEMETRY_PG_HOST \
    NAUTILUS_TELEMETRY_PG_PORT \
    NAUTILUS_TELEMETRY_PG_DATABASE \
    NAUTILUS_TELEMETRY_PG_SCHEMA \
    NAUTILUS_TELEMETRY_PG_USERNAME \
    NAUTILUS_TELEMETRY_PG_PASSWORD
  do
    if [[ -z "${!key:-}" ]]; then
      echo "[tokenmm-telemetry-shipper] missing required env ${key}" >&2
      missing=1
    fi
  done
  if [[ ${missing} -ne 0 ]]; then
    exit 78
  fi
}

main() {
  parse_args "$@"
  CONFIG_PATH="${CONFIG_PATH:-${WORKDIR}/deploy/tokenmm/tokenmm.live.toml}"
  if [[ -n "${SECRET_ID}" ]]; then
    load_secret_value
  fi
  require_pg_env
  cd "${WORKDIR}"
  exec "${WORKDIR}/.venv/bin/python" -m nautilus_trader.persistence.shipper.run --config "${CONFIG_PATH}" "${PASSTHROUGH_ARGS[@]}"
}

main "$@"
