#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="${ROOT_DIR:-$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)}"
source "${ROOT_DIR}/ops/scripts/deploy/shared_strategy_stack.sh"
SYSTEMD_DIR="${SYSTEMD_DIR:-${FLUX_SYSTEMD_DIR:-/etc/systemd/system}}"
ENV_DIR="${ENV_DIR:-${FLUX_ENV_DIR:-/etc/flux}}"
COMMON_ENV_PATH="${COMMON_ENV_PATH:-${FLUX_COMMON_ENV_PATH:-${ENV_DIR}/common.env}}"
DEPLOY_ROOT_OVERRIDE="${EQUITIES_DEPLOY_ROOT:-}"
DEPLOY_LANE="${EQUITIES_DEPLOY_LANE:-prod}"
TEST_MODE="${FLUX_DEPLOY_TEST_MODE:-0}"
ENABLE_EXECUTION="${EQUITIES_ENABLE_EXECUTION:-0}"
EQUITIES_API_PORT_OVERRIDE="${EQUITIES_API_PORT:-}"
EQUITIES_LANE_REDIS_DB_OVERRIDE="${EQUITIES_LANE_REDIS_DB:-}"

declare -a NODE_GROUP_LINES=()

STACK_SERVICE_PREFIX=""
PULSE_GROUP_KEY=""
PULSE_GROUP_LABEL=""
PULSE_GROUP_ORDER=""
EQUITIES_API_PORT=""
EQUITIES_REDIS_DB=""
TARGET_PATH=""
DEPLOY_ROOT=""
SHARED_CONFIG=""
STRATEGIES_DIR=""
EQUITIES_PYTHON_BIN=""
NODE_GROUP_HELPER=""

build_stack_service_prefix() {
  if [[ "${DEPLOY_LANE}" == "prod" ]]; then
    printf 'equities\n'
  else
    printf 'equities-%s\n' "${DEPLOY_LANE}"
  fi
}

build_group_label() {
  if [[ "${DEPLOY_LANE}" == "prod" ]]; then
    printf 'Equities\n'
  else
    printf 'Equities %s\n' "${DEPLOY_LANE^}"
  fi
}

default_group_order() {
  if [[ "${DEPLOY_LANE}" == "prod" ]]; then
    printf '20\n'
  else
    printf '21\n'
  fi
}

default_api_port() {
  if [[ "${DEPLOY_LANE}" == "prod" ]]; then
    printf '5024\n'
  else
    printf '5124\n'
  fi
}

default_redis_db() {
  if [[ "${DEPLOY_LANE}" == "prod" ]]; then
    printf '\n'
  else
    printf '1\n'
  fi
}

default_lane_release_root() {
  local releases_root="${RELEASES_ROOT:-${HOME}/releases}"
  printf '%s/%s/equities/current\n' "${releases_root}" "${DEPLOY_LANE}"
}

lane_api_env_path() {
  local stack_service_prefix="${STACK_SERVICE_PREFIX:-$(build_stack_service_prefix)}"
  printf '%s/%s-api.env\n' "${ENV_DIR}" "${stack_service_prefix}"
}

resolve_deploy_root() {
  local deploy_root=""
  local existing_api_env=""

  existing_api_env="$(lane_api_env_path)"
  if [[ -n "${DEPLOY_ROOT_OVERRIDE}" ]]; then
    deploy_root="${DEPLOY_ROOT_OVERRIDE}"
  elif [[ -f "${existing_api_env}" ]]; then
    deploy_root="$(strategy_stack_read_env_value "${existing_api_env}" "WORKDIR" || true)"
    if [[ -z "${deploy_root}" ]]; then
      deploy_root="$(strategy_stack_read_env_value "${existing_api_env}" "PYTHONPATH" || true)"
    fi
  elif [[ "${DEPLOY_LANE}" != "prod" ]]; then
    deploy_root="$(default_lane_release_root)"
  elif [[ -f "${COMMON_ENV_PATH}" ]]; then
    deploy_root="$(strategy_stack_read_env_value "${COMMON_ENV_PATH}" "WORKDIR" || true)"
    if [[ -z "${deploy_root}" ]]; then
      deploy_root="$(strategy_stack_read_env_value "${COMMON_ENV_PATH}" "PYTHONPATH" || true)"
    fi
  fi

  if [[ -z "${deploy_root}" ]]; then
    deploy_root="${ROOT_DIR}"
  fi

  printf '%s\n' "${deploy_root}"
}

initialize_stack_context() {
  strategy_stack_require_lane "${DEPLOY_LANE}"
  STACK_SERVICE_PREFIX="$(build_stack_service_prefix)"
  PULSE_GROUP_KEY="${STACK_SERVICE_PREFIX}"
  PULSE_GROUP_LABEL="$(build_group_label)"
  PULSE_GROUP_ORDER="$(default_group_order)"
  EQUITIES_API_PORT="${EQUITIES_API_PORT_OVERRIDE:-$(default_api_port)}"
  EQUITIES_REDIS_DB="${EQUITIES_LANE_REDIS_DB_OVERRIDE:-$(default_redis_db)}"
  TARGET_PATH="${SYSTEMD_DIR}/flux-${STACK_SERVICE_PREFIX}.target"
  DEPLOY_ROOT="$(resolve_deploy_root)"
  strategy_stack_require_immutable_release_root "${DEPLOY_ROOT}"
  SHARED_CONFIG="${DEPLOY_ROOT}/deploy/equities/equities.live.toml"
  STRATEGIES_DIR="${DEPLOY_ROOT}/deploy/equities/strategies"
  EQUITIES_PYTHON_BIN="${DEPLOY_ROOT}/.venv/bin/python"
  NODE_GROUP_HELPER="${DEPLOY_ROOT}/ops/scripts/deploy/list_equities_node_groups.py"
}

require_sudo() {
  if [[ "${TEST_MODE}" == "1" ]]; then
    return 0
  fi
  if [[ "${EUID}" -ne 0 ]]; then
    echo "[equities-systemd] run with sudo" >&2
    exit 1
  fi
}

require_project_python() {
  if [[ ! -x "${EQUITIES_PYTHON_BIN}" ]]; then
    echo "[equities-systemd] missing project python at ${EQUITIES_PYTHON_BIN}; run \`uv sync --all-groups --all-extras\` in ${DEPLOY_ROOT} first" >&2
    exit 1
  fi
}

discover_node_strategies() {
  discover_node_groups
}

discover_node_groups() {
  local discovered=()
  local helper_path="${NODE_GROUP_HELPER}"
  if [[ ! -f "${helper_path}" ]]; then
    helper_path="${ROOT_DIR}/ops/scripts/deploy/list_equities_node_groups.py"
  fi
  if [[ ! -f "${helper_path}" ]]; then
    echo "[equities-systemd] missing grouped-node helper at ${NODE_GROUP_HELPER}" >&2
    exit 1
  fi
  mapfile -t discovered < <(
    python3 "${helper_path}" \
      --shared-config "${SHARED_CONFIG}" \
      --strategies-dir "${STRATEGIES_DIR}"
  )
  if ((${#discovered[@]})); then
    NODE_GROUP_LINES=("${discovered[@]}")
  else
    NODE_GROUP_LINES=()
  fi
}

build_target_service_ids() {
  # shellcheck disable=SC2178
  local -n out_service_ids="$1"
  # shellcheck disable=SC2034
  out_service_ids=(
    "${STACK_SERVICE_PREFIX}-api"
    "${STACK_SERVICE_PREFIX}-portfolio"
    "${STACK_SERVICE_PREFIX}-bridge"
    "${STACK_SERVICE_PREFIX}-ibkr-reference-publisher"
  )
  local group_line=""
  local node_group_id=""
  for group_line in "${NODE_GROUP_LINES[@]}"; do
    IFS=$'\t' read -r node_group_id _ <<< "${group_line}"
    out_service_ids+=("${STACK_SERVICE_PREFIX}-node-${node_group_id}")
  done
}

install_units() {
  strategy_stack_install_base_units \
    "${DEPLOY_ROOT}" \
    "${SYSTEMD_DIR}" \
    "${ENV_DIR}" \
    "${DEPLOY_ROOT}/deploy/equities/systemd/common.env.example" \
    "${COMMON_ENV_PATH}"
}

append_deploy_root_env_overrides() {
  local env_path="$1"

  printf 'PYTHONDONTWRITEBYTECODE=1\n' >> "${env_path}"
  printf 'WORKDIR=%s\nPYTHONPATH=%s\n' "${DEPLOY_ROOT}" "${DEPLOY_ROOT}" >> "${env_path}"
  if [[ -n "${EQUITIES_REDIS_DB}" ]]; then
    printf 'EQUITIES_REDIS_DB=%s\n' "${EQUITIES_REDIS_DB}" >> "${env_path}"
  fi
}

cleanup_obsolete_envs() {
  rm -f "${ENV_DIR}/${STACK_SERVICE_PREFIX}-api.env"
  rm -f "${ENV_DIR}/${STACK_SERVICE_PREFIX}-portfolio.env"
  rm -f "${ENV_DIR}/${STACK_SERVICE_PREFIX}-bridge.env"
  rm -f "${ENV_DIR}/${STACK_SERVICE_PREFIX}-ibkr-reference-publisher.env"
  find "${ENV_DIR}" -maxdepth 1 -type f -name "${STACK_SERVICE_PREFIX}-node-*.env" -delete
}

render_target() {
  local service_ids=()
  build_target_service_ids service_ids
  if [[ "${DEPLOY_LANE}" == "prod" ]]; then
    strategy_stack_render_target "${TARGET_PATH}" "Flux Equities Stack" "${service_ids[@]}"
  else
    strategy_stack_render_target "${TARGET_PATH}" "Flux Equities ${DEPLOY_LANE^} Stack" "${service_ids[@]}"
  fi
}

render_api_env() {
  local api_service_id="${STACK_SERVICE_PREFIX}-api"

  # Shared-host equities keeps /equities as the SPA entry route; Fluxboard assets load from /static/fluxboard/*.
  strategy_stack_write_env \
    "${ENV_DIR}/${api_service_id}.env" \
    "Equities API backend" \
    "${PULSE_GROUP_KEY}" \
    "${PULSE_GROUP_LABEL}" \
    "${PULSE_GROUP_ORDER}" \
    "env FLUXBOARD_SERVE_DIST=1 ${EQUITIES_PYTHON_BIN} -m nautilus_trader.flux.runners.equities.run_api --config ${SHARED_CONFIG} --mode live --confirm-live --host 127.0.0.1 --port ${EQUITIES_API_PORT} --serve-fluxboard" \
    "${EQUITIES_API_PORT}" \
    "${api_service_id}" \
    "0"
  append_deploy_root_env_overrides "${ENV_DIR}/${api_service_id}.env"
}

render_portfolio_env() {
  strategy_stack_write_env \
    "${ENV_DIR}/${STACK_SERVICE_PREFIX}-portfolio.env" \
    "Equities portfolio aggregator" \
    "${PULSE_GROUP_KEY}" \
    "${PULSE_GROUP_LABEL}" \
    "${PULSE_GROUP_ORDER}" \
    "${EQUITIES_PYTHON_BIN} -m nautilus_trader.flux.runners.equities.run_portfolio --config ${SHARED_CONFIG} --mode live --confirm-live"
  append_deploy_root_env_overrides "${ENV_DIR}/${STACK_SERVICE_PREFIX}-portfolio.env"
}

render_bridge_env() {
  strategy_stack_write_env \
    "${ENV_DIR}/${STACK_SERVICE_PREFIX}-bridge.env" \
    "Equities bridge consumer" \
    "${PULSE_GROUP_KEY}" \
    "${PULSE_GROUP_LABEL}" \
    "${PULSE_GROUP_ORDER}" \
    "${EQUITIES_PYTHON_BIN} -m nautilus_trader.flux.runners.equities.run_bridge --config ${SHARED_CONFIG} --mode live --confirm-live"
  append_deploy_root_env_overrides "${ENV_DIR}/${STACK_SERVICE_PREFIX}-bridge.env"
}

render_publisher_env() {
  local publisher_service_id="${STACK_SERVICE_PREFIX}-ibkr-reference-publisher"

  strategy_stack_write_env \
    "${ENV_DIR}/${publisher_service_id}.env" \
    "Equities IBKR reference publisher" \
    "${PULSE_GROUP_KEY}" \
    "${PULSE_GROUP_LABEL}" \
    "${PULSE_GROUP_ORDER}" \
    "${EQUITIES_PYTHON_BIN} -m nautilus_trader.flux.runners.equities.run_ibkr_reference_publisher --config ${SHARED_CONFIG} --mode live --confirm-live" \
    "" \
    "${publisher_service_id}"
  append_deploy_root_env_overrides "${ENV_DIR}/${publisher_service_id}.env"
}

render_node_envs() {
  local group_line=""
  local group_parts=()
  local node_group_id=""
  local cmd=""
  local config_path=""
  local service_id=""
  for group_line in "${NODE_GROUP_LINES[@]}"; do
    IFS=$'\t' read -r -a group_parts <<< "${group_line}"
    node_group_id="${group_parts[0]}"
    service_id="${STACK_SERVICE_PREFIX}-node-${node_group_id}"
    cmd="${EQUITIES_PYTHON_BIN} -m nautilus_trader.flux.runners.equities.run_node"
    for config_path in "${group_parts[@]:1}"; do
      cmd+=" --config ${config_path}"
    done
    cmd+=" --shared-config ${SHARED_CONFIG} --mode live --confirm-live"
    if [[ "${ENABLE_EXECUTION}" == "1" ]]; then
      cmd+=" --enable-execution"
    fi
    strategy_stack_write_env \
      "${ENV_DIR}/${service_id}.env" \
      "Equities node ${node_group_id}" \
      "${PULSE_GROUP_KEY}" \
      "${PULSE_GROUP_LABEL}" \
      "${PULSE_GROUP_ORDER}" \
      "${cmd}"
    append_deploy_root_env_overrides "${ENV_DIR}/${service_id}.env"
  done
}

rebuild_pulse_sudoers() {
  "${DEPLOY_ROOT}/ops/scripts/deploy/rebuild_flux_pulse_sudoers.sh"
}

enable_stack() {
  if [[ "${TEST_MODE}" == "1" ]]; then
    return 0
  fi
  systemctl daemon-reload
  systemctl enable "flux-${STACK_SERVICE_PREFIX}.target"
}

main() {
  initialize_stack_context
  require_sudo
  require_project_python
  discover_node_groups
  install_units
  cleanup_obsolete_envs
  render_target
  render_api_env
  render_portfolio_env
  render_bridge_env
  render_publisher_env
  render_node_envs
  rebuild_pulse_sudoers
  enable_stack
  echo "[equities-systemd] installed units under ${SYSTEMD_DIR}, env files under ${ENV_DIR}, and sudoers at /etc/sudoers.d/flux-pulse"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
