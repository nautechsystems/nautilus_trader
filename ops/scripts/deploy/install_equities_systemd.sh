#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "${ROOT_DIR}/ops/scripts/deploy/shared_strategy_stack.sh"
SYSTEMD_DIR="/etc/systemd/system"
ENV_DIR="/etc/flux"
SUDOERS_DIR="/etc/sudoers.d"
SUDOERS_PATH="${SUDOERS_DIR}/flux-pulse"
COMMON_ENV_PATH="${ENV_DIR}/common.env"
TARGET_PATH="${SYSTEMD_DIR}/flux-equities.target"
SHARED_CONFIG="${ROOT_DIR}/deploy/equities/equities.live.toml"
STRATEGIES_DIR="${ROOT_DIR}/deploy/equities/strategies"

declare -a NODE_STRATEGIES=()

require_sudo() {
  if [[ "${EUID}" -ne 0 ]]; then
    echo "[equities-systemd] run with sudo" >&2
    exit 1
  fi
}

discover_node_strategies() {
  mapfile -t NODE_STRATEGIES < <(
    strategy_stack_discover_strategy_ids "${STRATEGIES_DIR}" "equities.strategy.template.toml"
  )
}

build_service_ids() {
  local -n out_service_ids="$1"
  out_service_ids=(
    "equities-portfolio"
    "equities-bridge"
  )
  local strategy_id
  for strategy_id in "${NODE_STRATEGIES[@]}"; do
    out_service_ids+=("equities-node-${strategy_id}")
  done
}


install_units() {
  strategy_stack_install_base_units \
    "${ROOT_DIR}" \
    "${SYSTEMD_DIR}" \
    "${ENV_DIR}" \
    "${ROOT_DIR}/deploy/equities/systemd/common.env.example" \
    "${COMMON_ENV_PATH}"
  install -d "${SUDOERS_DIR}"
}

cleanup_obsolete_envs() {
  rm -f "${ENV_DIR}/equities-api.env"
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

render_target() {
  local service_ids=()
  build_service_ids service_ids
  strategy_stack_render_target "${TARGET_PATH}" "Flux Equities Stack" "${service_ids[@]}"
}

render_portfolio_env() {
  strategy_stack_write_env \
    "${ENV_DIR}/equities-portfolio.env" \
    "Equities portfolio aggregator" \
    "equities" \
    "Equities" \
    "20" \
    "python3 -m nautilus_trader.flux.runners.equities.run_portfolio --config ${SHARED_CONFIG} --mode live --confirm-live"
}

render_bridge_env() {
  strategy_stack_write_env \
    "${ENV_DIR}/equities-bridge.env" \
    "Equities bridge consumer" \
    "equities" \
    "Equities" \
    "20" \
    "python3 -m nautilus_trader.flux.runners.equities.run_bridge --config ${SHARED_CONFIG} --mode live --confirm-live"
}

render_node_envs() {
  local strategy_id
  for strategy_id in "${NODE_STRATEGIES[@]}"; do
    strategy_stack_write_env \
      "${ENV_DIR}/equities-node-${strategy_id}.env" \
      "Equities node ${strategy_id}" \
      "equities" \
      "Equities" \
      "20" \
      "python3 -m nautilus_trader.flux.runners.equities.run_node --config ${STRATEGIES_DIR}/${strategy_id}.toml --shared-config ${SHARED_CONFIG} --mode live --confirm-live --enable-execution"
  done
}

enable_stack() {
  systemctl daemon-reload
  systemctl enable flux-equities.target
}

main() {
  require_sudo
  discover_node_strategies
  install_units
  cleanup_obsolete_envs
  install_sudoers
  render_target
  render_portfolio_env
  render_bridge_env
  render_node_envs
  enable_stack
  echo "[equities-systemd] installed units under /etc/systemd/system, env files under /etc/flux, and sudoers at /etc/sudoers.d/flux-pulse"
}

main "$@"
