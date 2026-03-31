#!/usr/bin/env bash
set -euo pipefail
: "${GRAFANA_ADMIN_PASSWORD:?GRAFANA_ADMIN_PASSWORD must be set}"
: "${TOKENMM_GRAFANA_INSTALL_DIR:=/opt/tokenmm-monitoring/grafana/current}"
: "${TOKENMM_GRAFANA_CONFIG_DIR:=/etc/tokenmm-monitoring/grafana}"
: "${TOKENMM_GRAFANA_DATA_DIR:=/var/lib/tokenmm-monitoring/grafana}"
: "${TOKENMM_GRAFANA_PORT:=3000}"

mkdir -p \
  "${TOKENMM_GRAFANA_DATA_DIR}" \
  "${TOKENMM_GRAFANA_DATA_DIR}/logs" \
  "${TOKENMM_GRAFANA_DATA_DIR}/plugins"

exec env \
  GF_PATHS_DATA="${TOKENMM_GRAFANA_DATA_DIR}" \
  GF_PATHS_LOGS="${TOKENMM_GRAFANA_DATA_DIR}/logs" \
  GF_PATHS_PLUGINS="${TOKENMM_GRAFANA_DATA_DIR}/plugins" \
  GF_PATHS_PROVISIONING="${TOKENMM_GRAFANA_CONFIG_DIR}/provisioning" \
  GF_LOG_MODE=console \
  GF_USERS_ALLOW_SIGN_UP=false \
  GF_SECURITY_ADMIN_PASSWORD="${GRAFANA_ADMIN_PASSWORD}" \
  GF_SERVER_HTTP_PORT="${TOKENMM_GRAFANA_PORT}" \
  "${TOKENMM_GRAFANA_INSTALL_DIR}/bin/grafana-server" \
  --homepath "${TOKENMM_GRAFANA_INSTALL_DIR}"
