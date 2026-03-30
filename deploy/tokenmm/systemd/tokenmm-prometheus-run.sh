#!/usr/bin/env bash
set -euo pipefail
: "${TOKENMM_PROMETHEUS_INSTALL_DIR:=/opt/tokenmm-monitoring/prometheus/current}"
: "${TOKENMM_PROMETHEUS_CONFIG_DIR:=/etc/tokenmm-monitoring/prometheus}"
: "${TOKENMM_PROMETHEUS_DATA_DIR:=/var/lib/tokenmm-monitoring/prometheus}"
: "${TOKENMM_PROMETHEUS_PORT:=9090}"

mkdir -p "${TOKENMM_PROMETHEUS_DATA_DIR}"

exec "${TOKENMM_PROMETHEUS_INSTALL_DIR}/prometheus" \
  --config.file="${TOKENMM_PROMETHEUS_CONFIG_DIR}/prometheus.yml" \
  --storage.tsdb.path="${TOKENMM_PROMETHEUS_DATA_DIR}" \
  --storage.tsdb.retention.time=30d \
  --web.console.templates="${TOKENMM_PROMETHEUS_INSTALL_DIR}/consoles" \
  --web.console.libraries="${TOKENMM_PROMETHEUS_INSTALL_DIR}/console_libraries" \
  --web.enable-lifecycle \
  --web.listen-address="0.0.0.0:${TOKENMM_PROMETHEUS_PORT}"
