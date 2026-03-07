#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "${ROOT_DIR}/ops/scripts/deploy/shared_strategy_stack.sh"
SYSTEMD_DIR="/etc/systemd/system"
ENV_DIR="/etc/flux"
SUDOERS_DIR="/etc/sudoers.d"
SUDOERS_PATH="${SUDOERS_DIR}/flux-pulse"
COMMON_ENV_PATH="${ENV_DIR}/common.env"
TARGET_PATH="${SYSTEMD_DIR}/flux-tokenmm.target"
SHARED_CONFIG="${ROOT_DIR}/deploy/tokenmm/tokenmm.live.toml"
STRATEGIES_DIR="${ROOT_DIR}/deploy/tokenmm/strategies"

declare -a NODE_STRATEGIES=()

require_sudo() {
  if [[ "${EUID}" -ne 0 ]]; then
    echo "[tokenmm-systemd] run with sudo" >&2
    exit 1
  fi
}

discover_node_strategies() {
  mapfile -t NODE_STRATEGIES < <(
    strategy_stack_discover_strategy_ids "${STRATEGIES_DIR}" "tokenmm.strategy.template.toml"
  )
}


build_service_ids() {
  local -n out_service_ids="$1"
  out_service_ids=(
    "tokenmm-api"
    "tokenmm-pulse"
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
  install -d "${SUDOERS_DIR}"
  install_sudoers
}

install_sudoers() {
  local tmp_sudoers
  local service_ids=()
  tmp_sudoers="$(mktemp)"
  build_service_ids service_ids
  strategy_stack_render_sudoers ubuntu "${tmp_sudoers}" "${service_ids[@]}"
  if command -v visudo >/dev/null 2>&1; then
    visudo -cf "${tmp_sudoers}"
  fi
  install -m 0440 "${tmp_sudoers}" "${SUDOERS_PATH}"
  rm -f "${tmp_sudoers}"
}

render_api_env() {
  strategy_stack_write_env \
    "${ENV_DIR}/tokenmm-api.env" \
    "TokenMM API + Fluxboard" \
    "tokenmm" \
    "TokenMM" \
    "10" \
    "env FLUXBOARD_SERVE_DIST=1 python3 -m nautilus_trader.flux.runners.tokenmm.run_api --config ${SHARED_CONFIG} --mode live --confirm-live --host 0.0.0.0 --port 5022 --serve-fluxboard" \
    "5022" \
    "tokenmm-api"
}

render_target() {
  local service_ids=()
  build_service_ids service_ids
  strategy_stack_render_target "${TARGET_PATH}" "Flux TokenMM Stack" "${service_ids[@]}"
}


render_pulse_env() {
  strategy_stack_write_env \
    "${ENV_DIR}/tokenmm-pulse.env" \
    "TokenMM Pulse" \
    "tokenmm" \
    "TokenMM" \
    "10" \
    "env PULSE_SERVE_DIST=1 python3 -m nautilus_trader.flux.runners.tokenmm.run_api --config ${SHARED_CONFIG} --mode live --confirm-live --host 127.0.0.1 --port 5023 --serve-pulse" \
    "5023" \
    "tokenmm-pulse"
}

render_portfolio_env() {
  strategy_stack_write_env \
    "${ENV_DIR}/tokenmm-portfolio.env" \
    "TokenMM portfolio aggregator" \
    "tokenmm" \
    "TokenMM" \
    "10" \
    "python3 -m nautilus_trader.flux.runners.tokenmm.run_portfolio --config ${SHARED_CONFIG} --mode live --confirm-live"
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
    "python3 -m nautilus_trader.flux.runners.tokenmm.run_bridge --config ${SHARED_CONFIG} --mode live --confirm-live${strategy_args}"
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
      "python3 -m nautilus_trader.flux.runners.tokenmm.run_node --config ${strategy_config} --shared-config ${SHARED_CONFIG} --mode live --confirm-live --enable-execution"
  done
}

enable_stack() {
  systemctl daemon-reload
  systemctl enable flux-tokenmm.target
}

main() {
  require_sudo
  discover_node_strategies
  install_units
  render_target
  render_api_env
  render_pulse_env
  render_portfolio_env
  render_bridge_env
  render_node_envs
  enable_stack
  echo "[tokenmm-systemd] installed units under /etc/systemd/system, env files under /etc/flux, and sudoers at /etc/sudoers.d/flux-pulse"
}

main "$@"
