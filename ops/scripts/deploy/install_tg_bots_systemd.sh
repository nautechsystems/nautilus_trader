#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="${ROOT_DIR:-$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)}"
source "${ROOT_DIR}/ops/scripts/deploy/shared_strategy_stack.sh"

SYSTEMD_DIR="${SYSTEMD_DIR:-/etc/systemd/system}"
ENV_DIR="${ENV_DIR:-/etc/flux}"
COMMON_ENV_PATH="${COMMON_ENV_PATH:-${ENV_DIR}/common.env}"
SERVICE_ID="${SERVICE_ID:-tg-bot-lan-rogue-trader-alert}"
SERVICE_ENV_PATH="${SERVICE_ENV_PATH:-${ENV_DIR}/${SERVICE_ID}.env}"
LOCAL_CONFIG_PATH="${LOCAL_CONFIG_PATH:-${ENV_DIR}/${SERVICE_ID}.ini}"
TARGET_PATH="${TARGET_PATH:-${SYSTEMD_DIR}/flux-tg-bots.target}"
SERVICE_ENV_OWNER="${SERVICE_ENV_OWNER-root:ubuntu}"
SERVICE_ENV_MODE="${SERVICE_ENV_MODE-0640}"
BINANCE_SECRET_ID="${LAN_ROGUE_TRADER_BOT_BINANCE_SECRET_ID:-/nautilus/tg-bots/lan_rogue_trader_bot/binance}"
TELEGRAM_SECRET_ID="${LAN_ROGUE_TRADER_BOT_TELEGRAM_SECRET_ID:-/nautilus/tg-bots/lan_rogue_trader_bot/telegram_bot}"
DEPLOY_ROOT_OVERRIDE="${TG_BOTS_DEPLOY_ROOT:-}"
DEPLOY_ROOT=""
TG_BOTS_PYTHON_BIN=""

declare -a MANAGED_SERVICE_ENV_KEYS=(
  "PULSE_ENABLED"
  "PULSE_DESCRIPTION"
  "PULSE_GROUP_KEY"
  "PULSE_GROUP_LABEL"
  "PULSE_GROUP_ORDER"
  "CMD"
  "LAN_ROGUE_TRADER_BOT_BINANCE_API_KEY"
  "LAN_ROGUE_TRADER_BOT_BINANCE_API_SECRET"
  "LAN_ROGUE_TRADER_BOT_TELEGRAM_BOT_TOKEN"
  "LAN_ROGUE_TRADER_BOT_BINANCE_SECRET_ID"
  "LAN_ROGUE_TRADER_BOT_TELEGRAM_SECRET_ID"
)


require_sudo() {
  if [[ "${EUID}" -ne 0 ]]; then
    echo "[tg-bots-systemd] run with sudo" >&2
    exit 1
  fi
}


resolve_deploy_root() {
  local deploy_root=""

  if [[ -n "${DEPLOY_ROOT_OVERRIDE}" ]]; then
    deploy_root="${DEPLOY_ROOT_OVERRIDE}"
  elif [[ -f "${SERVICE_ENV_PATH}" ]]; then
    deploy_root="$(strategy_stack_read_env_value "${SERVICE_ENV_PATH}" "WORKDIR" || true)"
    if [[ -z "${deploy_root}" ]]; then
      deploy_root="$(strategy_stack_read_env_value "${SERVICE_ENV_PATH}" "PYTHONPATH" || true)"
    fi
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
  DEPLOY_ROOT="$(resolve_deploy_root)"
  strategy_stack_require_immutable_release_root "${DEPLOY_ROOT}"
  TG_BOTS_PYTHON_BIN="${DEPLOY_ROOT}/.venv/bin/python"
}


install_units() {
  strategy_stack_install_base_units \
    "${DEPLOY_ROOT}" \
    "${SYSTEMD_DIR}" \
    "${ENV_DIR}" \
    "${DEPLOY_ROOT}/deploy/tg_bots/systemd/common.env.example" \
    "${COMMON_ENV_PATH}"
  install -m 0644 "${DEPLOY_ROOT}/deploy/tg_bots/systemd/flux-tg-bots.target" "${TARGET_PATH}"
  if [[ ! -f "${LOCAL_CONFIG_PATH}" ]]; then
    install -m 0644 "${DEPLOY_ROOT}/deploy/tg_bots/lan_rogue_trader_alert.ini" "${LOCAL_CONFIG_PATH}"
  fi
}


is_managed_service_env_key() {
  local candidate_key="$1"
  local managed_key=""
  for managed_key in "${MANAGED_SERVICE_ENV_KEYS[@]}"; do
    if [[ "${managed_key}" == "${candidate_key}" ]]; then
      return 0
    fi
  done
  return 1
}


preserve_existing_service_env_value() {
  local existing_env_path="$1"
  local key="$2"
  local fallback="$3"
  local value=""

  if [[ -n "${existing_env_path}" && -f "${existing_env_path}" ]]; then
    value="$(strategy_stack_read_env_value "${existing_env_path}" "${key}" || true)"
  fi

  if [[ -n "${value}" ]]; then
    printf '%s\n' "${value}"
    return 0
  fi

  printf '%s\n' "${fallback}"
}


append_unmanaged_service_env_lines() {
  local existing_env_path="$1"
  local line=""
  local trimmed_line=""
  local key=""

  if [[ -z "${existing_env_path}" || ! -f "${existing_env_path}" ]]; then
    return 0
  fi

  while IFS= read -r line || [[ -n "${line}" ]]; do
    trimmed_line="${line#"${line%%[![:space:]]*}"}"
    trimmed_line="${trimmed_line%"${trimmed_line##*[![:space:]]}"}"

    if [[ -z "${trimmed_line}" || "${trimmed_line}" == \#* ]]; then
      continue
    fi
    if [[ "${trimmed_line}" != *=* ]]; then
      continue
    fi

    key="${trimmed_line%%=*}"
    key="${key%"${key##*[![:space:]]}"}"
    if is_managed_service_env_key "${key}"; then
      continue
    fi

    printf '%s\n' "${trimmed_line}" >> "${SERVICE_ENV_PATH}"
  done < "${existing_env_path}"
}


append_deploy_root_env_overrides() {
  printf 'WORKDIR=%s\nPYTHONPATH=%s\n' "${DEPLOY_ROOT}" "${DEPLOY_ROOT}" >> "${SERVICE_ENV_PATH}"
}


render_service_env() {
  local existing_env_path=""
  local binance_api_key=""
  local binance_api_secret=""
  local telegram_bot_token=""
  local binance_secret_id=""
  local telegram_secret_id=""

  if [[ -f "${SERVICE_ENV_PATH}" ]]; then
    existing_env_path="$(mktemp)"
    cp "${SERVICE_ENV_PATH}" "${existing_env_path}"
  fi

  strategy_stack_write_env \
    "${SERVICE_ENV_PATH}" \
    "Lan Rogue Trader Telegram alert bot" \
    "tg-bots" \
    "TG Bots" \
    "60" \
    "${TG_BOTS_PYTHON_BIN} -m nautilus_trader.flux.runners.tg_bots.run_lan_rogue_trader_alert --config ${LOCAL_CONFIG_PATH}"

  binance_api_key="$(preserve_existing_service_env_value "${existing_env_path}" "LAN_ROGUE_TRADER_BOT_BINANCE_API_KEY" "${LAN_ROGUE_TRADER_BOT_BINANCE_API_KEY:-}")"
  binance_api_secret="$(preserve_existing_service_env_value "${existing_env_path}" "LAN_ROGUE_TRADER_BOT_BINANCE_API_SECRET" "${LAN_ROGUE_TRADER_BOT_BINANCE_API_SECRET:-}")"
  telegram_bot_token="$(preserve_existing_service_env_value "${existing_env_path}" "LAN_ROGUE_TRADER_BOT_TELEGRAM_BOT_TOKEN" "${LAN_ROGUE_TRADER_BOT_TELEGRAM_BOT_TOKEN:-}")"
  binance_secret_id="$(preserve_existing_service_env_value "${existing_env_path}" "LAN_ROGUE_TRADER_BOT_BINANCE_SECRET_ID" "${BINANCE_SECRET_ID}")"
  telegram_secret_id="$(preserve_existing_service_env_value "${existing_env_path}" "LAN_ROGUE_TRADER_BOT_TELEGRAM_SECRET_ID" "${TELEGRAM_SECRET_ID}")"

  {
    echo "LAN_ROGUE_TRADER_BOT_BINANCE_API_KEY=${binance_api_key}"
    echo "LAN_ROGUE_TRADER_BOT_BINANCE_API_SECRET=${binance_api_secret}"
    echo "LAN_ROGUE_TRADER_BOT_TELEGRAM_BOT_TOKEN=${telegram_bot_token}"
    echo "# Optional operator metadata only; the runtime does not auto-load these AWS Secrets Manager IDs."
    echo "LAN_ROGUE_TRADER_BOT_BINANCE_SECRET_ID=${binance_secret_id}"
    echo "LAN_ROGUE_TRADER_BOT_TELEGRAM_SECRET_ID=${telegram_secret_id}"
  } >> "${SERVICE_ENV_PATH}"
  append_unmanaged_service_env_lines "${existing_env_path}"
  append_deploy_root_env_overrides
  rm -f "${existing_env_path}"
  if [[ -n "${SERVICE_ENV_OWNER}" ]]; then
    chown "${SERVICE_ENV_OWNER}" "${SERVICE_ENV_PATH}"
  fi
  chmod "${SERVICE_ENV_MODE}" "${SERVICE_ENV_PATH}"
}


rebuild_pulse_sudoers() {
  "${DEPLOY_ROOT}/ops/scripts/deploy/rebuild_flux_pulse_sudoers.sh"
}


enable_stack() {
  systemctl daemon-reload
  systemctl enable flux-tg-bots.target
}


main() {
  initialize_stack_context
  require_sudo
  install_units
  render_service_env
  rebuild_pulse_sudoers
  enable_stack
  echo "[tg-bots-systemd] installed target under /etc/systemd/system, env under /etc/flux, and local config at ${LOCAL_CONFIG_PATH}"
}


if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  main "$@"
fi
