#!/usr/bin/env bash
set -euo pipefail

fail() {
  echo "[flux-host-baseline] $1" >&2
  exit 1
}

check_cloudwatch_agent_config() {
  python3 -m json.tool \
    /opt/aws/amazon-cloudwatch-agent/etc/amazon-cloudwatch-agent.d/flux-host.json \
    >/dev/null || fail "invalid CloudWatch Agent config"
}

check_docker_config() {
  if ! command -v docker >/dev/null 2>&1; then
    return
  fi

  if [[ ! -f /etc/docker/daemon.json ]]; then
    echo "[flux-host-baseline] warning: /etc/docker/daemon.json not present"
    return
  fi

  python3 - <<'PY' || fail "invalid Docker log rotation config"
import json
from pathlib import Path

config = json.loads(Path("/etc/docker/daemon.json").read_text())
if config.get("log-driver") != "json-file":
    raise SystemExit(1)

log_opts = config.get("log-opts", {})
if log_opts.get("max-size") != "50m":
    raise SystemExit(1)
if str(log_opts.get("max-file")) != "5":
    raise SystemExit(1)
PY
}

[[ -f /etc/systemd/journald.conf.d/99-flux-disk-limits.conf ]] || fail "missing journald drop-in"
[[ -f /opt/aws/amazon-cloudwatch-agent/etc/amazon-cloudwatch-agent.d/flux-host.json ]] || fail "missing CloudWatch Agent config"
[[ -f /etc/fluent-bit/flux-fluent-bit.yaml ]] || fail "missing Fluent Bit config"
check_cloudwatch_agent_config
check_docker_config

if command -v systemctl >/dev/null 2>&1; then
  systemctl is-active --quiet systemd-journald || fail "systemd-journald not active"
  if systemctl list-unit-files amazon-cloudwatch-agent.service >/dev/null 2>&1; then
    systemctl is-active --quiet amazon-cloudwatch-agent || fail "amazon-cloudwatch-agent not active"
  fi
  if systemctl list-unit-files fluent-bit.service >/dev/null 2>&1; then
    systemctl is-active --quiet fluent-bit || fail "fluent-bit not active"
  fi
fi

if command -v journalctl >/dev/null 2>&1; then
  journalctl --disk-usage
fi

echo "[flux-host-baseline] host baseline files present"
