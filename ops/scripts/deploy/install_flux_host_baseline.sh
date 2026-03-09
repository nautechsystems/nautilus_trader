#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../.." && pwd)"

require_sudo() {
  if [[ "${EUID}" -ne 0 ]]; then
    echo "[flux-host-baseline] run with sudo" >&2
    exit 1
  fi
}

install_journald_dropin() {
  install -d /etc/systemd/journald.conf.d
  install -m 0644 \
    "${ROOT_DIR}/deploy/host/journald/99-flux-disk-limits.conf" \
    /etc/systemd/journald.conf.d/99-flux-disk-limits.conf
  systemctl restart systemd-journald
}

install_docker_config() {
  install -d /etc/docker
  if [[ -f /etc/docker/daemon.json ]]; then
    echo "[flux-host-baseline] existing /etc/docker/daemon.json present; merge ${ROOT_DIR}/deploy/host/docker/daemon.json.example manually" >&2
    return
  fi

  install -m 0644 \
    "${ROOT_DIR}/deploy/host/docker/daemon.json.example" \
    /etc/docker/daemon.json
}

install_cloudwatch_agent_config() {
  install -d /opt/aws/amazon-cloudwatch-agent/etc/amazon-cloudwatch-agent.d
  install -m 0644 \
    "${ROOT_DIR}/deploy/aws/cloudwatch-agent/amazon-cloudwatch-agent.json" \
    /opt/aws/amazon-cloudwatch-agent/etc/amazon-cloudwatch-agent.d/flux-host.json
}

install_fluent_bit_config() {
  install -d /etc/fluent-bit
  install -m 0644 \
    "${ROOT_DIR}/deploy/aws/fluent-bit/fluent-bit.yaml" \
    /etc/fluent-bit/flux-fluent-bit.yaml
}

main() {
  require_sudo
  install_journald_dropin
  install_docker_config
  install_cloudwatch_agent_config
  install_fluent_bit_config
  echo "[flux-host-baseline] installed journald, Docker, CloudWatch Agent, and Fluent Bit baseline files"
}

main "$@"
