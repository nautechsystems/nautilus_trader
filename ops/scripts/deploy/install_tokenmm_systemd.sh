#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)"
SYSTEMD_DIR="/etc/systemd/system"
ENV_DIR="/etc/flux"
COMMON_ENV_PATH="${ENV_DIR}/common.env"
TARGET_PATH="${SYSTEMD_DIR}/flux-tokenmm.target"
DEPLOY_ROOT_OVERRIDE="${TOKENMM_DEPLOY_ROOT:-}"
TOKENMM_API_HOST="${TOKENMM_API_HOST:-}"

read_env_value() {
  local env_path="$1"
  local key="$2"
  local line=""

  [[ -f "${env_path}" ]] || return 1

  while IFS= read -r line || [[ -n "${line}" ]]; do
    line="${line#"${line%%[![:space:]]*}"}"
    line="${line%"${line##*[![:space:]]}"}"
    [[ -z "${line}" ]] && continue
    [[ "${line}" == \#* ]] && continue
    [[ "${line}" != "${key}"=* ]] && continue

    local value="${line#*=}"
    if [[ ${#value} -ge 2 && "${value:0:1}" == "${value: -1}" ]]; then
      case "${value:0:1}" in
        '"' | "'")
          value="${value:1:${#value}-2}"
          ;;
      esac
    fi
    printf '%s\n' "${value}"
    return 0
  done < "${env_path}"

  return 1
}

path_is_git_worktree() {
  local path="$1"
  local git_dir=""
  local common_dir=""

  git_dir="$(git -C "${path}" rev-parse --path-format=absolute --git-dir 2> /dev/null || true)"
  common_dir="$(
    git -C "${path}" rev-parse --path-format=absolute --git-common-dir 2> /dev/null || true
  )"

  [[ -n "${git_dir}" ]] || return 1
  [[ -n "${common_dir}" ]] || return 1
  [[ "${git_dir}" != "${common_dir}" ]]
}

resolve_deploy_root() {
  local deploy_root=""

  if [[ -n "${DEPLOY_ROOT_OVERRIDE}" ]]; then
    deploy_root="${DEPLOY_ROOT_OVERRIDE}"
  elif [[ -f "${COMMON_ENV_PATH}" ]]; then
    deploy_root="$(read_env_value "${COMMON_ENV_PATH}" "WORKDIR" || true)"
    if [[ -z "${deploy_root}" ]]; then
      deploy_root="$(read_env_value "${COMMON_ENV_PATH}" "PYTHONPATH" || true)"
    fi
  fi

  if [[ -z "${deploy_root}" ]]; then
    deploy_root="${ROOT_DIR}"
  fi

  printf '%s\n' "${deploy_root}"
}

read_existing_api_host() {
  local env_path="${ENV_DIR}/tokenmm-api.env"
  local cmd=""

  [[ -f "${env_path}" ]] || return 0

  cmd="$(read_env_value "${env_path}" "CMD" || true)"
  if [[ -n "${cmd}" && "${cmd}" =~ --host[[:space:]]+([^[:space:]]+) ]]; then
    printf '%s\n' "${BASH_REMATCH[1]}"
  fi
}

DEPLOY_ROOT="$(resolve_deploy_root)"
if [[ ! -d "${DEPLOY_ROOT}" ]]; then
  echo "[tokenmm-systemd] deploy root missing or not a directory: ${DEPLOY_ROOT}" >&2
  exit 1
fi
if path_is_git_worktree "${DEPLOY_ROOT}"; then
  echo "[tokenmm-systemd] deploy root must not be a git worktree: ${DEPLOY_ROOT}" >&2
  exit 1
fi
if [[ ! -f "${DEPLOY_ROOT}/ops/scripts/deploy/shared_strategy_stack.sh" ]]; then
  echo "[tokenmm-systemd] deploy root missing tokenmm deploy scripts: ${DEPLOY_ROOT}" >&2
  exit 1
fi

# shellcheck source=/dev/null
source "${DEPLOY_ROOT}/ops/scripts/deploy/shared_strategy_stack.sh"
SHARED_CONFIG="${DEPLOY_ROOT}/deploy/tokenmm/tokenmm.live.toml"
STRATEGIES_DIR="${DEPLOY_ROOT}/deploy/tokenmm/strategies"
TOKENMM_PYTHON_BIN="${DEPLOY_ROOT}/.venv/bin/python"

declare -a NODE_STRATEGIES=()

require_sudo() {
  if [[ "${EUID}" -ne 0 ]]; then
    echo "[tokenmm-systemd] run with sudo" >&2
    exit 1
  fi
}

require_project_python() {
  if [[ ! -x "${TOKENMM_PYTHON_BIN}" ]]; then
    echo "[tokenmm-systemd] missing project python at ${TOKENMM_PYTHON_BIN}; run \`uv sync --active --all-groups --all-extras\` in ${DEPLOY_ROOT} first" >&2
    exit 1
  fi
}

run_rollout_preflight() {
  "${TOKENMM_PYTHON_BIN}" "${DEPLOY_ROOT}/ops/scripts/deploy/tokenmm_rollout_preflight.py"
}

discover_node_strategies() {
  local discovered=""
  discovered="$(strategy_stack_discover_strategy_ids "${STRATEGIES_DIR}" "tokenmm.strategy.template.toml")"
  if [[ -n "${discovered}" ]]; then
    mapfile -t NODE_STRATEGIES <<< "${discovered}"
  else
    NODE_STRATEGIES=()
  fi
}

build_service_ids() {
  # shellcheck disable=SC2178
  local -n out_service_ids="$1"
  # shellcheck disable=SC2034
  out_service_ids=(
    "tokenmm-api"
    "tokenmm-portfolio"
    "tokenmm-bridge"
  )
  local strategy_id
  for strategy_id in "${NODE_STRATEGIES[@]}"; do
    out_service_ids+=("tokenmm-node-${strategy_id}")
  done
}

install_units() {
  strategy_stack_install_base_units \
    "${DEPLOY_ROOT}" \
    "${SYSTEMD_DIR}" \
    "${ENV_DIR}" \
    "${DEPLOY_ROOT}/deploy/tokenmm/systemd/common.env.example" \
    "${COMMON_ENV_PATH}"
}

append_deploy_root_env_overrides() {
  local env_path="$1"

  printf 'WORKDIR=%s\nPYTHONPATH=%s\n' "${DEPLOY_ROOT}" "${DEPLOY_ROOT}" >> "${env_path}"
}

render_api_env() {
  local api_host=""
  api_host="${TOKENMM_API_HOST:-$(read_existing_api_host)}"
  api_host="${api_host:-0.0.0.0}"

  strategy_stack_write_env \
    "${ENV_DIR}/tokenmm-api.env" \
    "TokenMM API + Fluxboard + Pulse" \
    "tokenmm" \
    "TokenMM" \
    "10" \
    "env FLUXBOARD_SERVE_DIST=1 PULSE_SERVE_DIST=1 ${TOKENMM_PYTHON_BIN} -m nautilus_trader.flux.runners.tokenmm.run_api --config ${SHARED_CONFIG} --mode live --confirm-live --host ${api_host} --port 5022 --serve-fluxboard --serve-pulse" \
    "5022" \
    "tokenmm-api"
  append_deploy_root_env_overrides "${ENV_DIR}/tokenmm-api.env"
}

render_target() {
  local service_ids=()
  build_service_ids service_ids
  strategy_stack_render_target "${TARGET_PATH}" "Flux TokenMM Stack" "${service_ids[@]}"
}

render_portfolio_env() {
  strategy_stack_write_env \
    "${ENV_DIR}/tokenmm-portfolio.env" \
    "TokenMM portfolio aggregator" \
    "tokenmm" \
    "TokenMM" \
    "10" \
    "${TOKENMM_PYTHON_BIN} -m nautilus_trader.flux.runners.tokenmm.run_portfolio --config ${SHARED_CONFIG} --mode live --confirm-live"
  append_deploy_root_env_overrides "${ENV_DIR}/tokenmm-portfolio.env"
}

render_bridge_env() {
  local strategy_args=""
  local strategy_id
  for strategy_id in "${NODE_STRATEGIES[@]}"; do
    strategy_args+=" --strategy-id ${strategy_id}"
  done
  strategy_stack_write_env \
    "${ENV_DIR}/tokenmm-bridge.env" \
    "TokenMM bridge consumer" \
    "tokenmm" \
    "TokenMM" \
    "10" \
    "${TOKENMM_PYTHON_BIN} -m nautilus_trader.flux.runners.tokenmm.run_bridge --config ${SHARED_CONFIG} --mode live --confirm-live${strategy_args}"
  append_deploy_root_env_overrides "${ENV_DIR}/tokenmm-bridge.env"
}

render_telemetry_shipper_env() {
  cat > "${ENV_DIR}/tokenmm-telemetry-shipper.env" << EOF
PULSE_ENABLED=1
PULSE_DESCRIPTION=TokenMM telemetry shipper
PULSE_GROUP_KEY=tokenmm
PULSE_GROUP_LABEL=TokenMM
PULSE_GROUP_ORDER=10
CMD="python3 -m nautilus_trader.persistence.shipper.run --config ${SHARED_CONFIG}"
EOF
}

render_node_envs() {
  local strategy_id
  for strategy_id in "${NODE_STRATEGIES[@]}"; do
    local service_id="tokenmm-node-${strategy_id}"
    local strategy_config="${STRATEGIES_DIR}/${strategy_id}.toml"
    strategy_stack_write_env \
      "${ENV_DIR}/${service_id}.env" \
      "TokenMM node ${strategy_id}" \
      "tokenmm" \
      "TokenMM" \
      "10" \
      "${TOKENMM_PYTHON_BIN} -m nautilus_trader.flux.runners.tokenmm.run_node --config ${strategy_config} --shared-config ${SHARED_CONFIG} --mode live --confirm-live --enable-execution"
    append_deploy_root_env_overrides "${ENV_DIR}/${service_id}.env"
  done
}

rebuild_pulse_sudoers() {
  "${DEPLOY_ROOT}/ops/scripts/deploy/rebuild_flux_pulse_sudoers.sh"
}

render_jupyter_env() {
  install -m 0640 \
    "${DEPLOY_ROOT}/deploy/tokenmm/systemd/tokenmm-jupyter.env.example" \
    "${ENV_DIR}/tokenmm-jupyter.env"
  append_deploy_root_env_overrides "${ENV_DIR}/tokenmm-jupyter.env"
}

enable_stack() {
  systemctl daemon-reload
  systemctl enable flux-tokenmm.target
}

main() {
  require_sudo
  require_project_python
  run_rollout_preflight
  discover_node_strategies
  install_units
  render_target
  render_api_env
  render_portfolio_env
  render_bridge_env
  render_telemetry_shipper_env
  render_node_envs
  rebuild_pulse_sudoers
  render_jupyter_env
  enable_stack
  echo "[tokenmm-systemd] installed units under /etc/systemd/system, env files under /etc/flux, and sudoers at /etc/sudoers.d/flux-pulse"
}

main "$@"
