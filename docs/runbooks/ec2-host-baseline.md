# EC2 Host Baseline

This baseline is for Flux production EC2 boxes.

## Required Host Controls

1. `journald` disk limits
   - install `deploy/host/journald/99-flux-disk-limits.conf`
   - verify with `systemd-analyze cat-config systemd/journald.conf`
2. Docker json-file log caps
   - use `deploy/host/docker/daemon.json.example`
   - merge carefully if `/etc/docker/daemon.json` already exists
3. CloudWatch Agent host metrics
   - install `deploy/aws/cloudwatch-agent/amazon-cloudwatch-agent.json`
   - publish root disk, inode, and memory metrics
4. Fluent Bit journal shipping
   - install `deploy/aws/fluent-bit/fluent-bit.yaml`
   - ship only `flux@*.service` journals to CloudWatch Logs

## Logging Contract

- On-host production source of truth is `journald`.
- Use `FLUX_LOG_LEVEL` as the shared default.
- Override only when necessary:
  - `FLUX_NODE_LOG_LEVEL`
  - `FLUX_BRIDGE_LOG_LEVEL`
  - `FLUX_PORTFOLIO_LOG_LEVEL`
  - `FLUX_API_LOG_LEVEL`
- Do not enable ad hoc file logging under `${WORKDIR}` on production boxes.

## Validation

Run:

```bash
sudo systemd-analyze cat-config systemd/journald.conf
sudo journalctl --disk-usage
sudo systemctl status amazon-cloudwatch-agent --no-pager
sudo systemctl status fluent-bit --no-pager
sudo docker info --format '{{.LoggingDriver}}'
```

Expected:

- journald limits present
- journal usage bounded
- host metric agent healthy
- Fluent Bit healthy when enabled
- Docker logging driver remains `json-file` with bounded rotation settings
