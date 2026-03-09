#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "${ROOT_DIR}/ops/scripts/deploy/shared_strategy_stack.sh"
SYSTEMD_DIR="/etc/systemd/system"
ENV_DIR="/etc/flux"
SUDOERS_DIR="/etc/sudoers.d"
SUDOERS_PATH="${SUDOERS_DIR}/flux-pulse"
COMMON_ENV_PATH="${ENV_DIR}/common.env"
TARGET_PATH="${SYSTEMD_DIR}/flux-lp.target"
API_CONFIG="${ROOT_DIR}/deploy/lp/lp.live.toml"
BAND1_CONFIG="${ROOT_DIR}/deploy/lp/hedgers/eth_plume_lp_hedger.ini"
BAND2_CONFIG="${ROOT_DIR}/deploy/lp/hedgers/eth_plume_lp_hedger_band2.ini"
HYPE_CONFIG="${ROOT_DIR}/deploy/lp/hedgers/hype_usdt_lp_hedger.ini"
PLUME_WETH_CONFIG="${ROOT_DIR}/deploy/lp/hedgers/plume_weth_lp_hedger.ini"
LP_API_ENV_PATH="${ENV_DIR}/lp-api.env"
BAND1_ENV_PATH="${ENV_DIR}/service-eth-plume-lp-hedger.env"
BAND2_ENV_PATH="${ENV_DIR}/service-eth-plume-lp-hedger-band2.env"
HYPE_ENV_PATH="${ENV_DIR}/service-hedger3.env"
PLUME_WETH_ENV_PATH="${ENV_DIR}/service-hedger4.env"

require_sudo() {
  if [[ "${EUID}" -ne 0 ]]; then
    echo "[lp-systemd] run with sudo" >&2
    exit 1
  fi
}

build_managed_service_ids() {
  # shellcheck disable=SC2178
  local -n out_service_ids="$1"
  # shellcheck disable=SC2034
  out_service_ids=(
    "lp-api"
    "service-eth-plume-lp-hedger"
    "service-eth-plume-lp-hedger-band2"
    "service-hedger3"
    "service-hedger4"
  )
}

build_target_service_ids() {
  # shellcheck disable=SC2178
  local -n out_service_ids="$1"
  # shellcheck disable=SC2034
  out_service_ids=(
    "lp-api"
    "service-eth-plume-lp-hedger"
    "service-eth-plume-lp-hedger-band2"
  )
}

install_units() {
  strategy_stack_install_base_units \
    "${ROOT_DIR}" \
    "${SYSTEMD_DIR}" \
    "${ENV_DIR}" \
    "${ROOT_DIR}/deploy/lp/systemd/common.env.example" \
    "${COMMON_ENV_PATH}"
  install -d "${SUDOERS_DIR}"
}

install_sudoers() {
  local tmp_sudoers
  local service_ids=()
  tmp_sudoers="$(mktemp)"
  build_managed_service_ids service_ids
  strategy_stack_render_merged_sudoers ubuntu "${tmp_sudoers}" "${SUDOERS_PATH}" "${service_ids[@]}"
  if command -v visudo > /dev/null 2>&1; then
    visudo -cf "${tmp_sudoers}"
  fi
  install -m 0440 "${tmp_sudoers}" "${SUDOERS_PATH}"
  rm -f "${tmp_sudoers}"
}

render_target() {
  local service_ids=()
  build_target_service_ids service_ids
  strategy_stack_render_target "${TARGET_PATH}" "Flux LP Stack" "${service_ids[@]}"
}

render_api_env() {
  strategy_stack_write_env \
    "${LP_API_ENV_PATH}" \
    "LP API backend" \
    "lp" \
    "LP" \
    "15" \
    "python3 -m lp.runners.run_api --config ${API_CONFIG} --host 127.0.0.1 --port 5025 --serve-fluxboard" \
    "5025" \
    "lp-api"
}

render_band1_env() {
  strategy_stack_write_env \
    "${BAND1_ENV_PATH}" \
    "ETH/PLUME LP Band1 hedger" \
    "lp" \
    "LP" \
    "15" \
    "python3 -m lp.runners.run_hedger --config ${BAND1_CONFIG} --system-config /etc/flux/lp-system.ini"
}

render_band2_env() {
  strategy_stack_write_env \
    "${BAND2_ENV_PATH}" \
    "ETH/PLUME LP Band2 hedger" \
    "lp" \
    "LP" \
    "15" \
    "python3 -m lp.runners.run_hedger --config ${BAND2_CONFIG} --system-config /etc/flux/lp-system.ini"
}

render_hype_env() {
  strategy_stack_write_env \
    "${HYPE_ENV_PATH}" \
    "Staged HYPE/USDT LP hedger" \
    "lp" \
    "LP" \
    "15" \
    "python3 -m lp.runners.run_hedger --config ${HYPE_CONFIG} --system-config /etc/flux/lp-system.ini"
}

render_plume_weth_env() {
  strategy_stack_write_env \
    "${PLUME_WETH_ENV_PATH}" \
    "Staged PLUME/WETH LP hedger" \
    "lp" \
    "LP" \
    "15" \
    "python3 -m lp.runners.run_hedger --config ${PLUME_WETH_CONFIG} --system-config /etc/flux/lp-system.ini"
}

enable_stack() {
  systemctl daemon-reload
  systemctl enable flux-lp.target
}

main() {
  require_sudo
  install_units
  install_sudoers
  render_target
  render_api_env
  render_band1_env
  render_band2_env
  render_hype_env
  render_plume_weth_env
  enable_stack
  echo "[lp-systemd] installed units under /etc/systemd/system, env files under /etc/flux, and sudoers at /etc/sudoers.d/flux-pulse"
  echo "[lp-systemd] staged generic hedgers service-hedger3 and service-hedger4 are Pulse-managed but not part of flux-lp.target"
  echo "[lp-systemd] restart flux@tokenmm-api.service after updating /etc/flux/common.env so LP_API_BACKEND_URL is reloaded before starting flux-lp.target"
}

main "$@"
