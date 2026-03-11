#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "${ROOT_DIR}/ops/scripts/deploy/shared_strategy_stack.sh"
SYSTEMD_DIR="/etc/systemd/system"
ENV_DIR="/etc/flux"
COMMON_ENV_PATH="${ENV_DIR}/common.env"
TARGET_PATH="${SYSTEMD_DIR}/flux-tokenmm.target"
SHARED_CONFIG="${ROOT_DIR}/deploy/tokenmm/tokenmm.live.toml"
STRATEGIES_DIR="${ROOT_DIR}/deploy/tokenmm/strategies"
TOKENMM_PYTHON_BIN="${ROOT_DIR}/.venv/bin/python"

declare -a NODE_STRATEGIES=()

require_sudo() {
  if [[ "${EUID}" -ne 0 ]]; then
    echo "[tokenmm-systemd] run with sudo" >&2
    exit 1
  fi
}

require_project_python() {
  if [[ ! -x "${TOKENMM_PYTHON_BIN}" ]]; then
    echo "[tokenmm-systemd] missing project python at ${TOKENMM_PYTHON_BIN}; run \`uv sync --active --all-groups --all-extras\` first" >&2
    exit 1
  fi
}

run_rollout_preflight() {
  "${TOKENMM_PYTHON_BIN}" "${ROOT_DIR}/ops/scripts/deploy/tokenmm_rollout_preflight.py"
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
    "${ROOT_DIR}" \
    "${SYSTEMD_DIR}" \
    "${ENV_DIR}" \
    "${ROOT_DIR}/deploy/tokenmm/systemd/common.env.example" \
    "${COMMON_ENV_PATH}"
}

append_checkout_env_overrides() {
  local env_path="$1"

  printf 'WORKDIR=%s\nPYTHONPATH=%s\n' "${ROOT_DIR}" "${ROOT_DIR}" >> "${env_path}"
}

render_api_env() {
  strategy_stack_write_env \
    "${ENV_DIR}/tokenmm-api.env" \
    "TokenMM API + Fluxboard + Pulse" \
    "tokenmm" \
    "TokenMM" \
    "10" \
    "env FLUXBOARD_SERVE_DIST=1 PULSE_SERVE_DIST=1 ${TOKENMM_PYTHON_BIN} -m nautilus_trader.flux.runners.tokenmm.run_api --config ${SHARED_CONFIG} --mode live --confirm-live --host 127.0.0.1 --port 5022 --serve-fluxboard --serve-pulse" \
    "5022" \
    "tokenmm-api"
  append_checkout_env_overrides "${ENV_DIR}/tokenmm-api.env"
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
  append_checkout_env_overrides "${ENV_DIR}/tokenmm-portfolio.env"
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
  append_checkout_env_overrides "${ENV_DIR}/tokenmm-bridge.env"
}

render_telemetry_shipper_env() {
  cat > "${ENV_DIR}/tokenmm-telemetry-shipper.env" <<EOF
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
    append_checkout_env_overrides "${ENV_DIR}/${service_id}.env"
  done
}

rebuild_pulse_sudoers() {
  "${ROOT_DIR}/ops/scripts/deploy/rebuild_flux_pulse_sudoers.sh"
}

render_jupyter_env() {
  install -m 0640 \
    "${ROOT_DIR}/deploy/tokenmm/systemd/tokenmm-jupyter.env.example" \
    "${ENV_DIR}/tokenmm-jupyter.env"
  append_checkout_env_overrides "${ENV_DIR}/tokenmm-jupyter.env"
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
