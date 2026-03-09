#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "${ROOT_DIR}/ops/scripts/deploy/shared_strategy_stack.sh"

SYSTEMD_DIR="/etc/systemd/system"
ENV_DIR="/etc/flux"
COMMON_ENV_PATH="${ENV_DIR}/common.env"
SERVICE_ID="tg-bot-lan-rogue-trader-alert"
SERVICE_ENV_PATH="${ENV_DIR}/${SERVICE_ID}.env"
LOCAL_CONFIG_PATH="${ENV_DIR}/${SERVICE_ID}.ini"
TARGET_PATH="${SYSTEMD_DIR}/flux-tg-bots.target"
BINANCE_SECRET_ID="${LAN_ROGUE_TRADER_BOT_BINANCE_SECRET_ID:-/nautilus/tg-bots/lan_rogue_trader_bot/binance}"
TELEGRAM_SECRET_ID="${LAN_ROGUE_TRADER_BOT_TELEGRAM_SECRET_ID:-/nautilus/tg-bots/lan_rogue_trader_bot/telegram_bot}"


require_sudo() {
  if [[ "${EUID}" -ne 0 ]]; then
    echo "[tg-bots-systemd] run with sudo" >&2
    exit 1
  fi
}


install_units() {
  strategy_stack_install_base_units \
    "${ROOT_DIR}" \
    "${SYSTEMD_DIR}" \
    "${ENV_DIR}" \
    "${ROOT_DIR}/deploy/tg_bots/systemd/common.env.example" \
    "${COMMON_ENV_PATH}"
  install -m 0644 "${ROOT_DIR}/deploy/tg_bots/systemd/flux-tg-bots.target" "${TARGET_PATH}"
  if [[ ! -f "${LOCAL_CONFIG_PATH}" ]]; then
    install -m 0644 "${ROOT_DIR}/deploy/tg_bots/lan_rogue_trader_alert.ini" "${LOCAL_CONFIG_PATH}"
  fi
}


render_service_env() {
  strategy_stack_write_env \
    "${SERVICE_ENV_PATH}" \
    "Lan Rogue Trader Telegram alert bot" \
    "tg-bots" \
    "TG Bots" \
    "60" \
    "python3 -m nautilus_trader.flux.runners.tg_bots.run_lan_rogue_trader_alert --config ${LOCAL_CONFIG_PATH}"

  {
    echo "LAN_ROGUE_TRADER_BOT_BINANCE_API_KEY="
    echo "LAN_ROGUE_TRADER_BOT_BINANCE_API_SECRET="
    echo "LAN_ROGUE_TRADER_BOT_TELEGRAM_BOT_TOKEN="
    echo "LAN_ROGUE_TRADER_BOT_BINANCE_SECRET_ID=${BINANCE_SECRET_ID}"
    echo "LAN_ROGUE_TRADER_BOT_TELEGRAM_SECRET_ID=${TELEGRAM_SECRET_ID}"
  } >> "${SERVICE_ENV_PATH}"
  chown root:ubuntu "${SERVICE_ENV_PATH}"
  chmod 0640 "${SERVICE_ENV_PATH}"
}


rebuild_pulse_sudoers() {
  "${ROOT_DIR}/ops/scripts/deploy/rebuild_flux_pulse_sudoers.sh"
}


enable_stack() {
  systemctl daemon-reload
  systemctl enable flux-tg-bots.target
}


main() {
  require_sudo
  install_units
  render_service_env
  rebuild_pulse_sudoers
  enable_stack
  echo "[tg-bots-systemd] installed target under /etc/systemd/system, env under /etc/flux, and local config at ${LOCAL_CONFIG_PATH}"
}


main "$@"
