#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
SYSTEMD_DIR="/etc/systemd/system"
ENV_DIR="/etc/flux"
SUDOERS_DIR="/etc/sudoers.d"
SUDOERS_PATH="${SUDOERS_DIR}/flux-pulse"
COMMON_ENV_PATH="${ENV_DIR}/common.env"
SHARED_CONFIG="${ROOT_DIR}/deploy/tokenmm/tokenmm.live.toml"

declare -a NODE_STRATEGIES=(
  "plumeusdt_bybit_perp_makerv3"
  "plumeusdt_bybit_spot_makerv3"
  "plumeusdt_okx_perp_makerv3"
  "plumeusdt_binance_spot_makerv3"
)

require_sudo() {
  if [[ "${EUID}" -ne 0 ]]; then
    echo "[tokenmm-systemd] run with sudo" >&2
    exit 1
  fi
}

install_units() {
  install -d "${SYSTEMD_DIR}" "${ENV_DIR}" "${SUDOERS_DIR}"
  install -m 0644 "${ROOT_DIR}/deploy/systemd/flux@.service" "${SYSTEMD_DIR}/flux@.service"
  install -m 0644 "${ROOT_DIR}/deploy/tokenmm/systemd/flux-tokenmm.target" "${SYSTEMD_DIR}/flux-tokenmm.target"
  if [[ ! -f "${COMMON_ENV_PATH}" ]]; then
    install -m 0640 "${ROOT_DIR}/deploy/tokenmm/systemd/common.env.example" "${COMMON_ENV_PATH}"
  fi
  install_sudoers
}

install_sudoers() {
  local tmp_sudoers
  tmp_sudoers="$(mktemp)"
  install -m 0440 "${ROOT_DIR}/deploy/tokenmm/systemd/flux-pulse.sudoers" "${tmp_sudoers}"
  if command -v visudo >/dev/null 2>&1; then
    visudo -cf "${tmp_sudoers}"
  fi
  install -m 0440 "${tmp_sudoers}" "${SUDOERS_PATH}"
  rm -f "${tmp_sudoers}"
}

render_api_env() {
  cat > "${ENV_DIR}/tokenmm-api.env" <<EOF
PULSE_ENABLED=1
PULSE_DESCRIPTION=TokenMM API + Fluxboard + Pulse
PULSE_GROUP_KEY=tokenmm
PULSE_GROUP_LABEL=TokenMM
PULSE_GROUP_ORDER=10
PULSE_SELF_SERVICE_ID=tokenmm-api
PORT=5022
CMD="env FLUXBOARD_SERVE_DIST=1 PULSE_SERVE_DIST=1 python3 -m nautilus_trader.flux.runners.tokenmm.run_api --config ${SHARED_CONFIG} --mode live --confirm-live --host 0.0.0.0 --port 5022 --serve-fluxboard --serve-pulse"
EOF
}

render_portfolio_env() {
  cat > "${ENV_DIR}/tokenmm-portfolio.env" <<EOF
PULSE_ENABLED=1
PULSE_DESCRIPTION=TokenMM portfolio aggregator
PULSE_GROUP_KEY=tokenmm
PULSE_GROUP_LABEL=TokenMM
PULSE_GROUP_ORDER=10
CMD="python3 -m nautilus_trader.flux.runners.tokenmm.run_portfolio --config ${SHARED_CONFIG} --mode live --confirm-live"
EOF
}

render_bridge_env() {
  cat > "${ENV_DIR}/tokenmm-bridge.env" <<EOF
PULSE_ENABLED=1
PULSE_DESCRIPTION=TokenMM bridge consumer
PULSE_GROUP_KEY=tokenmm
PULSE_GROUP_LABEL=TokenMM
PULSE_GROUP_ORDER=10
CMD="python3 -m nautilus_trader.flux.runners.tokenmm.run_bridge --config ${SHARED_CONFIG} --mode live --confirm-live --all-strategies"
EOF
}

render_node_envs() {
  for strategy_id in "${NODE_STRATEGIES[@]}"; do
    local service_id="tokenmm-node-${strategy_id}"
    local strategy_config="${ROOT_DIR}/deploy/tokenmm/strategies/${strategy_id}.toml"
    cat > "${ENV_DIR}/${service_id}.env" <<EOF
PULSE_ENABLED=1
PULSE_DESCRIPTION=TokenMM node ${strategy_id}
PULSE_GROUP_KEY=tokenmm
PULSE_GROUP_LABEL=TokenMM
PULSE_GROUP_ORDER=10
CMD="python3 -m nautilus_trader.flux.runners.tokenmm.run_node --config ${strategy_config} --shared-config ${SHARED_CONFIG} --mode live --confirm-live --enable-execution"
EOF
  done
}

enable_stack() {
  systemctl daemon-reload
  systemctl enable flux-tokenmm.target
}

main() {
  require_sudo
  install_units
  render_api_env
  render_portfolio_env
  render_bridge_env
  render_node_envs
  enable_stack
  echo "[tokenmm-systemd] installed units under /etc/systemd/system, env files under /etc/flux, and sudoers at /etc/sudoers.d/flux-pulse"
}

main "$@"
