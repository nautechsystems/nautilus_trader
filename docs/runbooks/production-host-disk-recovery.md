# Production Host Disk Recovery

This runbook records the March 9, 2026 disk recovery performed on the shared EC2 host.

## Before

- Root volume: `629G`
- Used: `542G`
- Free: `88G`
- Usage: `87%`

Largest consumers at the time:

- `/home/ubuntu/chainsaw/logs` about `131G`
- `/home/ubuntu/nautilus_trader/.worktrees` about `158G`
- `/home/ubuntu/nautilus-trader-dev/.worktrees` about `147G`
- `/var/lib/docker` about `12G`
- `/var/log/journal` about `456M`

Worst single files at the time:

- `/home/ubuntu/chainsaw/logs/strategy_runner_shard-0.log` about `111G`
- `/home/ubuntu/chainsaw/logs/bybit_service.log` about `8.1G`
- `/home/ubuntu/chainsaw/logs/strategy_runner_shard-2.log` about `6.9G`
- `/home/ubuntu/chainsaw/logs/eth_plume_lp_hedger.log` about `2.5G`

## Safety Checks

Confirmed before deletion/truncation:

- `chainsaw` services were inactive or failed, not running.
- `lsof` showed no active writers for the large `chainsaw` log files.
- Active Flux production services were `flux@*` units, not `chainsaw@*`.

## Cleanup Performed

Removed decommissioned `chainsaw` artifacts:

- `/home/ubuntu/chainsaw/logs`
- `/home/ubuntu/chainsaw/.worktrees`
- `/home/ubuntu/chainsaw/worktrees`
- `/home/ubuntu/chainsaw/.venv`
- `/home/ubuntu/chainsaw/venv`
- `/home/ubuntu/chainsaw/nexus`

Removed Rust build outputs from non-current worktrees and repo roots:

- `/home/ubuntu/nautilus_trader/target`
- `/home/ubuntu/nautilus_trader/build`
- `/home/ubuntu/nautilus-trader-dev/target`
- `/home/ubuntu/nautilus-trader-dev/build`
- all `target` and `build` directories under:
  - `/home/ubuntu/nautilus_trader/.worktrees`, excluding the active `makerv3-mono-pr` workspace
  - `/home/ubuntu/nautilus-trader-dev/.worktrees`
  - `/home/ubuntu/.worktrees/nautilus_trader`
  - `/home/ubuntu/.config/superpowers/worktrees/nautilus_trader`

Removed stopped Docker containers with names matching `chainsaw-*`.

## After

- Root volume: `629G`
- Used: `122G`
- Free: `507G`
- Usage: `20%`

Remaining notable consumers after cleanup:

- `/home/ubuntu/nautilus_trader/.worktrees` about `69G`
- `/var/lib/docker` about `12G`
- `/home/ubuntu/chainsaw` about `973M`
- `/home/ubuntu/nautilus-trader-dev/.worktrees` about `3.8G`
- `/home/ubuntu/.config/superpowers/worktrees/nautilus_trader` about `2.2G`
- `/home/ubuntu/.worktrees` about `1.9G`
- `/var/log/journal` about `456M`

## Follow-up

- Keep `chainsaw` treated as decommissioned. Do not add new retention or monitoring work for it.
- The remaining `/home/ubuntu/chainsaw` source tree was not deleted because active shells and a Codex process still had that directory as their current working directory during cleanup. Remove it after those sessions exit if full decommission cleanup is required.
- If more space is needed later, inspect the remaining `nautilus_trader` worktrees and remove inactive source worktrees, not just build outputs.
- Flux production logging should remain journal-first and be managed separately from this decommission cleanup.
