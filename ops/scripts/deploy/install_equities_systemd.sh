#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "${ROOT_DIR}/ops/scripts/deploy/shared_strategy_stack.sh"
SYSTEMD_DIR="/etc/systemd/system"
ENV_DIR="/etc/flux"
COMMON_ENV_PATH="${ENV_DIR}/common.env"
TARGET_PATH="${SYSTEMD_DIR}/flux-equities.target"
SHARED_CONFIG="${ROOT_DIR}/deploy/equities/equities.live.toml"
STRATEGIES_DIR="${ROOT_DIR}/deploy/equities/strategies"
EQUITIES_PYTHON_BIN="${ROOT_DIR}/.venv/bin/python"

declare -a NODE_STRATEGIES=()

require_sudo() {
  if [[ "${EUID}" -ne 0 ]]; then
    echo "[equities-systemd] run with sudo" >&2
    exit 1
  fi
}

require_project_python() {
  if [[ ! -x "${EQUITIES_PYTHON_BIN}" ]]; then
    echo "[equities-systemd] missing project python at ${EQUITIES_PYTHON_BIN}; run \`uv sync --all-groups --all-extras\` first" >&2
    exit 1
  fi
}

discover_node_strategies() {
  local discovered=""
  discovered="$(strategy_stack_discover_strategy_ids "${STRATEGIES_DIR}" "equities.strategy.template.toml")"
  if [[ -n "${discovered}" ]]; then
    mapfile -t NODE_STRATEGIES <<< "${discovered}"
  else
    NODE_STRATEGIES=()
  fi
}

build_target_service_ids() {
  # shellcheck disable=SC2178
  local -n out_service_ids="$1"
  # shellcheck disable=SC2034
  out_service_ids=(
    "equities-api"
    "equities-portfolio"
    "equities-bridge"
  )
  local strategy_id
  for strategy_id in "${NODE_STRATEGIES[@]}"; do
    out_service_ids+=("equities-node-${strategy_id}")
  done
}

build_pulse_service_ids() {
  # shellcheck disable=SC2178
  local -n out_service_ids="$1"
  # shellcheck disable=SC2034,SC2178
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
}

append_checkout_env_overrides() {
  local env_path="$1"

  printf 'WORKDIR=%s\nPYTHONPATH=%s\n' "${ROOT_DIR}" "${ROOT_DIR}" >> "${env_path}"
}

cleanup_obsolete_envs() {
  rm -f "${ENV_DIR}/equities-api.env"
  find "${ENV_DIR}" -maxdepth 1 -type f -name 'equities-node-*.env' -delete
}

render_target() {
  local service_ids=()
  build_target_service_ids service_ids
  strategy_stack_render_target "${TARGET_PATH}" "Flux Equities Stack" "${service_ids[@]}"
}

render_api_env() {
  # Shared-host equities keeps /equities as the SPA entry route; Fluxboard assets load from /static/fluxboard/*.
  strategy_stack_write_env \
    "${ENV_DIR}/equities-api.env" \
    "Equities API backend" \
    "equities" \
    "Equities" \
    "20" \
    "env FLUXBOARD_SERVE_DIST=1 ${EQUITIES_PYTHON_BIN} -m nautilus_trader.flux.runners.equities.run_api --config ${SHARED_CONFIG} --mode live --confirm-live --host 127.0.0.1 --port 5024 --serve-fluxboard" \
    "5024" \
    "" \
    "0"
  append_checkout_env_overrides "${ENV_DIR}/equities-api.env"
}

render_portfolio_env() {
  strategy_stack_write_env \
    "${ENV_DIR}/equities-portfolio.env" \
    "Equities portfolio aggregator" \
    "equities" \
    "Equities" \
    "20" \
    "${EQUITIES_PYTHON_BIN} -m nautilus_trader.flux.runners.equities.run_portfolio --config ${SHARED_CONFIG} --mode live --confirm-live"
  append_checkout_env_overrides "${ENV_DIR}/equities-portfolio.env"
}

render_bridge_env() {
  strategy_stack_write_env \
    "${ENV_DIR}/equities-bridge.env" \
    "Equities bridge consumer" \
    "equities" \
    "Equities" \
    "20" \
    "${EQUITIES_PYTHON_BIN} -m nautilus_trader.flux.runners.equities.run_bridge --config ${SHARED_CONFIG} --mode live --confirm-live"
  append_checkout_env_overrides "${ENV_DIR}/equities-bridge.env"
}

render_node_envs() {
  local strategy_id
  for strategy_id in "${NODE_STRATEGIES[@]}"; do
    local service_id="equities-node-${strategy_id}"
    strategy_stack_write_env \
      "${ENV_DIR}/${service_id}.env" \
      "Equities node ${strategy_id}" \
      "equities" \
      "Equities" \
      "20" \
      "${EQUITIES_PYTHON_BIN} -m nautilus_trader.flux.runners.equities.run_node --config ${STRATEGIES_DIR}/${strategy_id}.toml --shared-config ${SHARED_CONFIG} --mode live --confirm-live --enable-execution"
    append_checkout_env_overrides "${ENV_DIR}/${service_id}.env"
  done
}

rebuild_pulse_sudoers() {
  "${ROOT_DIR}/ops/scripts/deploy/rebuild_flux_pulse_sudoers.sh"
}

enable_stack() {
  systemctl daemon-reload
  systemctl enable flux-equities.target
}

main() {
  require_sudo
  require_project_python
  discover_node_strategies
  install_units
  cleanup_obsolete_envs
  render_target
  render_api_env
  render_portfolio_env
  render_bridge_env
  render_node_envs
  rebuild_pulse_sudoers
  enable_stack
  echo "[equities-systemd] installed units under /etc/systemd/system, env files under /etc/flux, and sudoers at /etc/sudoers.d/flux-pulse"
}

main "$@"
