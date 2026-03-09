# EC2 Log and Disk Rollout

Roll out the Flux host baseline one box at a time.

## Canary Sequence

1. Install the host baseline files:

```bash
sudo ops/scripts/deploy/install_flux_host_baseline.sh
```

2. Verify conformance:

```bash
ops/scripts/deploy/check_flux_host_baseline.sh
```

3. Confirm runtime state:

```bash
df -h
sudo journalctl --disk-usage
sudo systemctl list-units --type=service --state=running 'flux@*' --no-pager
```

4. Confirm CloudWatch:

- root disk usage metric present
- inode metric present
- memory metric present
- Flux journals visible in the configured CloudWatch Logs group

## Alert Thresholds

- warning: root disk usage above `75%`
- critical: root disk usage above `85%`
- warning: low inode headroom
- critical: repeated `flux@` restarts

## Rollout Order

1. one non-critical EC2 box
2. shared production host
3. remaining production boxes

## Notes

- `chainsaw` is decommissioned and intentionally out of scope for this baseline.
- If a host still needs more headroom after cleanup, expand EBS before the next risky rollout.
