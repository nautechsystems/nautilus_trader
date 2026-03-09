#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)"
source "${ROOT_DIR}/ops/scripts/deploy/shared_strategy_stack.sh"

ENV_DIR="${ENV_DIR:-/etc/flux}"
SUDOERS_PATH="${SUDOERS_PATH:-/etc/sudoers.d/flux-pulse}"
RUN_AS_USER="${RUN_AS_USER:-ubuntu}"
VISUDO_BIN="${VISUDO_BIN:-}"


resolve_visudo() {
  if [[ -n "${VISUDO_BIN}" ]]; then
    printf '%s\n' "${VISUDO_BIN}"
    return 0
  fi
  command -v visudo || true
}


rebuild_flux_pulse_sudoers() {
  local service_ids=()
  strategy_stack_collect_pulse_service_ids service_ids "${ENV_DIR}"

  if ((${#service_ids[@]} == 0)); then
    rm -f "${SUDOERS_PATH}"
    echo "[flux-pulse-sudoers] removed ${SUDOERS_PATH}; no PULSE_ENABLED jobs found under ${ENV_DIR}"
    return 0
  fi

  local tmp_sudoers
  tmp_sudoers="$(mktemp)"
  strategy_stack_render_sudoers "${RUN_AS_USER}" "${tmp_sudoers}" "${service_ids[@]}"

  local resolved_visudo=""
  resolved_visudo="$(resolve_visudo)"
  if [[ -n "${resolved_visudo}" ]]; then
    "${resolved_visudo}" -cf "${tmp_sudoers}" > /dev/null
  fi

  install -d "$(dirname "${SUDOERS_PATH}")"
  install -m 0440 "${tmp_sudoers}" "${SUDOERS_PATH}"
  rm -f "${tmp_sudoers}"
  echo "[flux-pulse-sudoers] rebuilt ${SUDOERS_PATH} from ${#service_ids[@]} enrolled job(s)"
}


main() {
  rebuild_flux_pulse_sudoers
}


main "$@"
