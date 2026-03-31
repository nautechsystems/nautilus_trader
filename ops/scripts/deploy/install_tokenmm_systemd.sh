#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)"
SYSTEMD_DIR="/etc/systemd/system"
ENV_DIR="/etc/flux"
COMMON_ENV_PATH="${ENV_DIR}/common.env"
TARGET_PATH="${SYSTEMD_DIR}/flux-tokenmm.target"
DEPLOY_ROOT_OVERRIDE="${TOKENMM_DEPLOY_ROOT:-}"
TOKENMM_API_HOST="${TOKENMM_API_HOST:-}"
TOKENMM_GRAFANA_ADMIN_PASSWORD="${TOKENMM_GRAFANA_ADMIN_PASSWORD:-}"
TELEMETRY_HEALTH_SERVICE_PATH="${SYSTEMD_DIR}/flux-tokenmm-telemetry-health.service"
TELEMETRY_HEALTH_TIMER_PATH="${SYSTEMD_DIR}/flux-tokenmm-telemetry-health.timer"
MONITORING_ROOT="/etc/tokenmm-monitoring"
MONITORING_RUNTIME_ROOT="/opt/tokenmm-monitoring"
MONITORING_EXPORTER_DIR="${MONITORING_ROOT}/exporter"
MONITORING_GRAFANA_DASHBOARDS_DIR="${MONITORING_ROOT}/grafana/dashboards"
MONITORING_GRAFANA_PROVISIONING_DASHBOARDS_DIR="${MONITORING_ROOT}/grafana/provisioning/dashboards"
MONITORING_GRAFANA_PROVISIONING_DATASOURCES_DIR="${MONITORING_ROOT}/grafana/provisioning/datasources"
MONITORING_PROMETHEUS_DIR="${MONITORING_ROOT}/prometheus"
MONITORING_GRAFANA_RUNTIME_ROOT="${MONITORING_RUNTIME_ROOT}/grafana"
MONITORING_PROMETHEUS_RUNTIME_ROOT="${MONITORING_RUNTIME_ROOT}/prometheus"
MONITORING_GRAFANA_DATA_DIR="/var/lib/tokenmm-monitoring/grafana"
MONITORING_PROMETHEUS_DATA_DIR="/var/lib/tokenmm-monitoring/prometheus"
GRAFANA_WRAPPER_PATH="/usr/local/bin/tokenmm-grafana-run.sh"
PROMETHEUS_WRAPPER_PATH="/usr/local/bin/tokenmm-prometheus-run.sh"
GRAFANA_VERSION="10.2.3"
PROMETHEUS_VERSION="2.48.0"

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

read_existing_grafana_admin_password() {
  local env_path="${ENV_DIR}/tokenmm-grafana.env"

  [[ -f "${env_path}" ]] || return 0
  read_env_value "${env_path}" "GRAFANA_ADMIN_PASSWORD" || true
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

resolve_grafana_admin_password() {
  local password="${TOKENMM_GRAFANA_ADMIN_PASSWORD}"

  if [[ -z "${password}" ]]; then
    password="$(read_existing_grafana_admin_password)"
  fi

  if [[ -z "${password}" ]]; then
    password="$("${TOKENMM_PYTHON_BIN}" - <<'PY'
import secrets
import string

alphabet = string.ascii_letters + string.digits
print("".join(secrets.choice(alphabet) for _ in range(24)))
PY
)"
  fi

  printf '%s\n' "${password}"
}

build_service_ids() {
  # shellcheck disable=SC2178
  local -n out_service_ids="$1"
  # shellcheck disable=SC2034
  out_service_ids=(
    "tokenmm-api"
    "tokenmm-controller"
    "tokenmm-portfolio"
    "tokenmm-bridge"
    "tokenmm-telemetry-shipper"
    "tokenmm-prometheus"
    "tokenmm-grafana"
    "tokenmm-liquidity-exporter"
    "tokenmm-markouts-exporter"
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
  # flux-tokenmm-telemetry-health.service executes ops/scripts/deploy/tokenmm_telemetry_healthcheck.py.
  install -m 0644 \
    "${DEPLOY_ROOT}/deploy/tokenmm/systemd/flux-tokenmm-telemetry-health.service" \
    "${TELEMETRY_HEALTH_SERVICE_PATH}"
  install -m 0644 \
    "${DEPLOY_ROOT}/deploy/tokenmm/systemd/flux-tokenmm-telemetry-health.timer" \
    "${TELEMETRY_HEALTH_TIMER_PATH}"
  install -m 0640 \
    "${DEPLOY_ROOT}/deploy/tokenmm/systemd/tokenmm-telemetry-rds.env.example" \
    "${ENV_DIR}/tokenmm-telemetry-rds.env.example"
}

install_monitoring_assets() {
  install -d \
    "${MONITORING_GRAFANA_RUNTIME_ROOT}" \
    "${MONITORING_PROMETHEUS_RUNTIME_ROOT}" \
    "${MONITORING_EXPORTER_DIR}" \
    "${MONITORING_GRAFANA_DASHBOARDS_DIR}" \
    "${MONITORING_GRAFANA_PROVISIONING_DASHBOARDS_DIR}" \
    "${MONITORING_GRAFANA_PROVISIONING_DATASOURCES_DIR}" \
    "${MONITORING_PROMETHEUS_DIR}"
  install -d -o ubuntu -g ubuntu \
    "${MONITORING_GRAFANA_DATA_DIR}" \
    "${MONITORING_PROMETHEUS_DATA_DIR}"

  install -m 0755 "${DEPLOY_ROOT}/deploy/tokenmm/systemd/tokenmm-grafana-run.sh" "${GRAFANA_WRAPPER_PATH}"
  install -m 0755 "${DEPLOY_ROOT}/deploy/tokenmm/systemd/tokenmm-prometheus-run.sh" "${PROMETHEUS_WRAPPER_PATH}"
  install -m 0644 "${DEPLOY_ROOT}/deploy/tokenmm/systemd/prometheus.yml" "${MONITORING_PROMETHEUS_DIR}/prometheus.yml"
  install -m 0644 \
    "${DEPLOY_ROOT}/monitoring/grafana/provisioning/dashboards/dashboards.yml" \
    "${MONITORING_GRAFANA_PROVISIONING_DASHBOARDS_DIR}/dashboards.yml"
  install -m 0644 \
    "${DEPLOY_ROOT}/monitoring/grafana/provisioning/datasources/datasources.yml" \
    "${MONITORING_GRAFANA_PROVISIONING_DATASOURCES_DIR}/datasources.yml"

  local dashboard_path=""
  for dashboard_path in "${DEPLOY_ROOT}"/monitoring/grafana/dashboards/*.json; do
    install -m 0644 "${dashboard_path}" "${MONITORING_GRAFANA_DASHBOARDS_DIR}/$(basename "${dashboard_path}")"
  done
}

append_deploy_root_env_overrides() {
  local env_path="$1"

  printf 'WORKDIR=%s\nPYTHONPATH=%s\n' "${DEPLOY_ROOT}" "${DEPLOY_ROOT}" >> "${env_path}"
}

append_env_var() {
  local env_path="$1"
  local key="$2"
  local value="$3"

  printf '%s=%s\n' "${key}" "${value}" >> "${env_path}"
}

resolve_monitoring_archive_arch() {
  case "$(uname -m)" in
    x86_64 | amd64)
      printf 'amd64\n'
      ;;
    aarch64 | arm64)
      printf 'arm64\n'
      ;;
    *)
      echo "[tokenmm-systemd] unsupported monitoring archive architecture: $(uname -m)" >&2
      exit 1
      ;;
  esac
}

download_and_extract_monitoring_tarball() {
  local url="$1"
  local destination="$2"
  local tmpdir=""

  (
    tmpdir="$(mktemp -d)"
    trap 'rm -rf -- "${tmpdir}"' EXIT
    install -d "$(dirname "${destination}")"
    rm -rf "${destination}"
    install -d "${destination}"
    curl -fsSL "${url}" -o "${tmpdir}/bundle.tar.gz"
    tar -xzf "${tmpdir}/bundle.tar.gz" -C "${destination}" --strip-components=1
    chown -R root:root "${destination}"
  )
}

install_monitoring_binaries() {
  local archive_arch=""
  local grafana_version_dir=""
  local prometheus_version_dir=""

  archive_arch="$(resolve_monitoring_archive_arch)"
  grafana_version_dir="${MONITORING_GRAFANA_RUNTIME_ROOT}/grafana-v${GRAFANA_VERSION}"
  prometheus_version_dir="${MONITORING_PROMETHEUS_RUNTIME_ROOT}/prometheus-${PROMETHEUS_VERSION}.linux-${archive_arch}"

  if [[ ! -x "${grafana_version_dir}/bin/grafana-server" ]]; then
    download_and_extract_monitoring_tarball \
      "https://dl.grafana.com/oss/release/grafana-${GRAFANA_VERSION}.linux-${archive_arch}.tar.gz" \
      "${grafana_version_dir}"
  fi
  if [[ ! -x "${prometheus_version_dir}/prometheus" ]]; then
    download_and_extract_monitoring_tarball \
      "https://github.com/prometheus/prometheus/releases/download/v${PROMETHEUS_VERSION}/prometheus-${PROMETHEUS_VERSION}.linux-${archive_arch}.tar.gz" \
      "${prometheus_version_dir}"
  fi

  ln -sfn "${grafana_version_dir}" "${MONITORING_GRAFANA_RUNTIME_ROOT}/current"
  ln -sfn "${prometheus_version_dir}" "${MONITORING_PROMETHEUS_RUNTIME_ROOT}/current"
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

render_controller_env() {
  # controller-owned shared Binance writer domains stay on the controller lane
  # so binance.pm.main startup reconciliation ownership stays with the controller.
  strategy_stack_write_env \
    "${ENV_DIR}/tokenmm-controller.env" \
    "TokenMM shared Binance controller" \
    "tokenmm" \
    "TokenMM" \
    "10" \
    "${TOKENMM_PYTHON_BIN} -m nautilus_trader.flux.runners.tokenmm.run_controller --config ${SHARED_CONFIG} --mode live --confirm-live" \
    "" \
    "tokenmm-controller"
  printf 'TOKENMM_CONTROLLER_SCOPE_ID=tokenmm.binance.pm.main\n' >> "${ENV_DIR}/tokenmm-controller.env"
  printf 'TOKENMM_CONTROLLER_ACCOUNT_SCOPE_ID=binance.pm.main\n' >> "${ENV_DIR}/tokenmm-controller.env"
  append_deploy_root_env_overrides "${ENV_DIR}/tokenmm-controller.env"
}

render_telemetry_shipper_env() {
  strategy_stack_write_env \
    "${ENV_DIR}/tokenmm-telemetry-shipper.env" \
    "TokenMM telemetry shipper" \
    "tokenmm" \
    "TokenMM" \
    "10" \
    "${DEPLOY_ROOT}/ops/scripts/deploy/run_tokenmm_telemetry_shipper.sh --config ${SHARED_CONFIG}"
  append_deploy_root_env_overrides "${ENV_DIR}/tokenmm-telemetry-shipper.env"
}

render_prometheus_env() {
  strategy_stack_write_env \
    "${ENV_DIR}/tokenmm-prometheus.env" \
    "TokenMM Prometheus" \
    "tokenmm" \
    "TokenMM" \
    "10" \
    "${PROMETHEUS_WRAPPER_PATH}" \
    "9090" \
    "tokenmm-prometheus"
  append_deploy_root_env_overrides "${ENV_DIR}/tokenmm-prometheus.env"
}

render_grafana_env() {
  local grafana_admin_password=""
  grafana_admin_password="$(resolve_grafana_admin_password)"

  strategy_stack_write_env \
    "${ENV_DIR}/tokenmm-grafana.env" \
    "TokenMM Grafana" \
    "tokenmm" \
    "TokenMM" \
    "10" \
    "${GRAFANA_WRAPPER_PATH}" \
    "3000" \
    "tokenmm-grafana"
  append_env_var "${ENV_DIR}/tokenmm-grafana.env" "GRAFANA_ADMIN_PASSWORD" "${grafana_admin_password}"
  append_deploy_root_env_overrides "${ENV_DIR}/tokenmm-grafana.env"
}

render_liquidity_exporter_env() {
  strategy_stack_write_env \
    "${ENV_DIR}/tokenmm-liquidity-exporter.env" \
    "TokenMM liquidity metrics exporter" \
    "tokenmm" \
    "TokenMM" \
    "10" \
    "${TOKENMM_PYTHON_BIN} ${DEPLOY_ROOT}/ops/scripts/exporters/tokenmm_metrics_exporter.py --env prod --port 9108 --poll-interval-s 5 --strategy-group tokenmm" \
    "9108" \
    "tokenmm-liquidity-exporter"
  append_deploy_root_env_overrides "${ENV_DIR}/tokenmm-liquidity-exporter.env"
}

render_markouts_exporter_env() {
  strategy_stack_write_env \
    "${ENV_DIR}/tokenmm-markouts-exporter.env" \
    "TokenMM markouts metrics exporter" \
    "tokenmm" \
    "TokenMM" \
    "10" \
    "${TOKENMM_PYTHON_BIN} ${DEPLOY_ROOT}/ops/scripts/exporters/tokenmm_markouts_exporter.py --env prod --profile tokenmm --fills-db /var/lib/nautilus/telemetry/tokenmm/fills.sqlite --markouts-db /var/lib/nautilus/telemetry/tokenmm/markouts.sqlite --benchmark-name fv_market_mid,local_mkt_mid --window-hours 168 --port 9109 --poll-interval-s 15" \
    "9109" \
    "tokenmm-markouts-exporter"
  append_deploy_root_env_overrides "${ENV_DIR}/tokenmm-markouts-exporter.env"
}

render_node_envs() {
  local strategy_id
  for strategy_id in "${NODE_STRATEGIES[@]}"; do
    local service_id="tokenmm-node-${strategy_id}"
    local strategy_config="${STRATEGIES_DIR}/${strategy_id}.toml"
    local controller_managed_strategy=0
    local exec_flag=()
    case "${strategy_id}" in
      plumeusdt_binance_perp_makerv3|plumeusdt_binance_spot_makerv3)
        controller_managed_strategy=1
        ;;
    esac
    if [[ "${controller_managed_strategy}" -eq 0 ]]; then
      exec_flag+=(--enable-execution)
    fi
    strategy_stack_write_env \
      "${ENV_DIR}/${service_id}.env" \
      "TokenMM node ${strategy_id}" \
      "tokenmm" \
      "TokenMM" \
      "10" \
      "${TOKENMM_PYTHON_BIN} -m nautilus_trader.flux.runners.tokenmm.run_node --config ${strategy_config} --shared-config ${SHARED_CONFIG} --mode live --confirm-live ${exec_flag[*]}"
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
  systemctl enable flux-tokenmm-telemetry-health.timer
}

main() {
  require_sudo
  require_project_python
  run_rollout_preflight
  discover_node_strategies
  install_units
  install_monitoring_binaries
  install_monitoring_assets
  render_target
  render_api_env
  render_portfolio_env
  render_bridge_env
  render_controller_env
  render_telemetry_shipper_env
  render_prometheus_env
  render_grafana_env
  render_liquidity_exporter_env
  render_markouts_exporter_env
  render_node_envs
  rebuild_pulse_sudoers
  render_jupyter_env
  enable_stack
  echo "[tokenmm-systemd] installed units under /etc/systemd/system, env files under /etc/flux, and sudoers at /etc/sudoers.d/flux-pulse"
}

main "$@"
