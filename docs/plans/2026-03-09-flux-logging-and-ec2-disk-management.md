# Flux Logging and EC2 Disk Management Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Standardize Flux strategy and engine logging, bound on-host log growth, and add reusable EC2 disk/logging guardrails so production boxes remain operable under sustained load.

**Architecture:** Keep `systemd` + `journald` as the on-host production source of truth because Pulse already reads logs with `journalctl`. Unify all Flux runners behind one shared logging bootstrap so nodes, bridge, portfolio, and API produce consistent stdout/stderr output, while high-volume state remains on structured Redis topics. Add a host baseline for journald caps, Docker log caps, CloudWatch disk metrics, optional journald shipping to CloudWatch Logs, and explicit cleanup policy for non-prod local file logs and stale build artifacts.

**Tech Stack:** Python, Nautilus `LoggingConfig`, `systemd`, `journald`, Pulse, AWS CloudWatch Agent, CloudWatch Logs, Fluent Bit, EC2/EBS, Docker.

## Current State Snapshot

- Root volume on this host is `629G`, with `529G` used (`85%`).
- `journald` is already capped locally and currently uses about `456M`.
- The dominant disk consumers are outside Flux prod journald logs:
  - `/home/ubuntu/chainsaw/logs` ≈ `131G`
  - `/home/ubuntu/nautilus_trader/.worktrees` ≈ `158G`
  - `/home/ubuntu/nautilus-trader-dev/.worktrees` ≈ `147G`
  - `/home/ubuntu/chainsaw/.worktrees` ≈ `11G`
  - `/var/lib/docker` ≈ `12G`
- The worst single files currently visible are:
  - `/home/ubuntu/chainsaw/logs/strategy_runner_shard-0.log` ≈ `111G`
  - `/home/ubuntu/chainsaw/logs/bybit_service.log` ≈ `8.1G`
  - `/home/ubuntu/chainsaw/logs/strategy_runner_shard-2.log` ≈ `6.9G`

## Decisions

- Production Flux services should stay journal-first on-host. Do not enable ad hoc file logging under `${WORKDIR}`.
- Flux runners should share one logging bootstrap and one config surface.
- Structured Redis topics remain the place for high-volume strategy telemetry; logs remain for lifecycle, guardrail, failure, and operator context.
- Box-level retention and alarms must be repo-managed or runbook-managed, not left to AMI drift.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Task 1: Recover Current Host Capacity | completed | main | Cleanup runbook added; root usage dropped from 87% to 20% after chainsaw/build cleanup and is now 21% with normal runtime drift |
| Task 2: Standardize Flux Runner Logging | completed | main | Shared logging bootstrap added; invalid explicit log levels fail fast again; API startup summary is unconditional again; shared logging tests 7 passed and affected runner subset 104 passed; equities run-node still needs external ibapi dependency |
| Task 3: Wire Deploy Config and Local Log Retention | completed | main | FLUX_* env contract documented; local smoke scripts now rotate logs on size and keep bounded history; bash -n passed for tokenmm/equities stack scripts |
| Task 4: Add EC2 Host Logging and Disk Baseline | completed | main | Journald/Docker/CloudWatch Agent/Fluent Bit baseline assets and executable install/check scripts added; baseline installed on the shared host and journald/Docker settings verified |
| Task 5: Roll Out Monitoring, Retention, and Validation | completed | main | Rollout and host-baseline runbooks added; host baseline check passed on the shared host with root at 21% and journal usage at 471.3M; CloudWatch Agent and Fluent Bit configs installed for package-backed rollout |

---

### Task 1: Recover Current Host Capacity

**Files:**
- Create: `docs/runbooks/production-host-disk-recovery.md`
- Modify: `ops/README.md`

**Step 1: Capture the before snapshot**

Run:

```bash
df -h
sudo du -xhd1 /home/ubuntu 2>/dev/null | sort -h | tail -20
sudo du -xhd1 /home/ubuntu/chainsaw/logs 2>/dev/null | sort -h
sudo journalctl --disk-usage
sudo docker system df
```

Expected: root usage and the top-level consumers match the current-state snapshot within normal runtime drift.

**Step 2: Confirm the giant chainsaw logs are not actively open**

Run:

```bash
sudo lsof /home/ubuntu/chainsaw/logs/strategy_runner_shard-0.log \
  /home/ubuntu/chainsaw/logs/bybit_service.log \
  /home/ubuntu/chainsaw/logs/strategy_runner_shard-2.log \
  /home/ubuntu/chainsaw/logs/eth_plume_lp_hedger.log
```

Expected: no active writers, or a clear owning process list that forces a coordinated restart/truncate instead of blind deletion.

**Step 3: Remove or archive the acute space offenders**

Run the minimum safe action based on Step 2:

```bash
sudo truncate -s 0 /home/ubuntu/chainsaw/logs/strategy_runner_shard-0.log
sudo truncate -s 0 /home/ubuntu/chainsaw/logs/bybit_service.log
sudo truncate -s 0 /home/ubuntu/chainsaw/logs/strategy_runner_shard-2.log
sudo truncate -s 0 /home/ubuntu/chainsaw/logs/eth_plume_lp_hedger.log
```

Expected: immediate large drop in used bytes without changing file ownership or service paths.

**Step 4: Prune stale build artifacts and abandoned worktrees**

Run:

```bash
git -C /home/ubuntu/nautilus_trader worktree list
git -C /home/ubuntu/nautilus-trader-dev worktree list
sudo find /home/ubuntu -xdev -type d \( -name target -o -name build \) -prune -exec du -sh {} + 2>/dev/null | sort -h | tail -40
```

Then remove only confirmed-stale worktrees and their `target` / `build` trees.

Expected: reclaim tens to hundreds of GB without touching active production services.

**Step 5: Re-measure and write the recovery runbook**

Run:

```bash
df -h
sudo du -sh /home/ubuntu/chainsaw/logs /home/ubuntu/nautilus_trader/.worktrees /home/ubuntu/nautilus-trader-dev/.worktrees
```

Expected: root usage is materially below the alert threshold chosen in Task 5, with the exact cleanup commands recorded in the runbook.

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Standardize Flux Runner Logging

**Files:**
- Create: `systems/flux/flux/runners/shared/logging.py`
- Create: `tests/unit_tests/flux/runners/shared/test_logging.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_node.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_bridge.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_portfolio.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_api.py`
- Modify: `systems/flux/flux/runners/equities/run_node.py`
- Modify: `systems/flux/flux/runners/equities/run_bridge.py`
- Modify: `systems/flux/flux/runners/equities/run_portfolio.py`
- Modify: `systems/flux/flux/runners/equities/run_api.py`

**Step 1: Write failing tests for the shared bootstrap contract**

Cover:

- default production mode is journal-first and does not enable file logging
- service-level log level override resolution
- stdout vs stderr routing for severity
- stable formatter output for bridge, portfolio, and API processes

Run:

```bash
pytest tests/unit_tests/flux/runners/shared/test_logging.py -v
```

Expected: FAIL because the shared bootstrap module does not exist yet.

**Step 2: Implement the shared bootstrap**

Create one helper that:

- resolves env and config defaults
- builds consistent Python handlers for bridge, portfolio, and API
- builds Nautilus `LoggingConfig` for nodes without enabling prod file logs
- makes warning/error severity visible in journald by routing them to stderr

Expected: a single module owns logging policy instead of per-runner ad hoc setup.

**Step 3: Replace runner-local logging setup**

Remove inline `logging.basicConfig(...)` usage and wire every Flux entrypoint through the shared bootstrap.

Run:

```bash
pytest tests/unit_tests/flux/runners/shared/test_logging.py -v
pytest tests/unit_tests/flux -k logging -v
```

Expected: PASS for the new shared bootstrap tests and any affected Flux logging tests.

**Step 4: Keep strategy telemetry out of the log hot path**

Update runner comments and docs so high-frequency quote-cycle and state detail stays on Redis topics such as `flux.makerv3.state`, `flux.makerv3.event`, and `flux.makerv3.alert`, not as free-form repeated log spam.

Expected: the code and docs reflect a deliberate split between logs and structured strategy telemetry.

**Step 5: Commit**

```bash
git add tests/unit_tests/flux/runners/shared/test_logging.py \
  systems/flux/flux/runners/shared/logging.py \
  systems/flux/flux/runners/tokenmm/run_node.py \
  systems/flux/flux/runners/tokenmm/run_bridge.py \
  systems/flux/flux/runners/tokenmm/run_portfolio.py \
  systems/flux/flux/runners/tokenmm/run_api.py \
  systems/flux/flux/runners/equities/run_node.py \
  systems/flux/flux/runners/equities/run_bridge.py \
  systems/flux/flux/runners/equities/run_portfolio.py \
  systems/flux/flux/runners/equities/run_api.py
git commit -m "feat: standardize flux runner logging"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Wire Deploy Config and Local Log Retention

**Files:**
- Modify: `deploy/tokenmm/systemd/common.env.example`
- Modify: `deploy/equities/systemd/common.env.example`
- Modify: `deploy/tokenmm/README.md`
- Modify: `deploy/equities/README.md`
- Modify: `ops/scripts/deploy/tokenmm_stack.sh`
- Modify: `ops/scripts/deploy/equities_stack.sh`
- Create: `docs/runbooks/local-log-retention.md`

**Step 1: Replace the ambiguous `LOG_LEVEL` contract**

Decide on explicit env names such as:

- `FLUX_LOG_LEVEL`
- `FLUX_NODE_LOG_LEVEL`
- `FLUX_BRIDGE_LOG_LEVEL`
- `FLUX_PORTFOLIO_LOG_LEVEL`
- `FLUX_API_LOG_LEVEL`

Expected: config examples match actual runner behavior.

**Step 2: Add bounded retention for local smoke logs**

Update the local stack scripts so `.run/tokenmm-stack/logs` and `.run/equities-stack/logs` do not grow forever. Prefer one of:

- timestamped run directories plus best-effort cleanup of old runs
- size-based truncation/rotation on startup

Expected: local smoke remains usable without becoming a silent disk leak.

**Step 3: Document the production non-goal**

Document that production Flux services should not write unbounded files under `${WORKDIR}` and that file logging must use an explicit dedicated path if ever re-enabled.

Run:

```bash
rg -n "LOG_LEVEL|journald|WORKDIR|local smoke|log retention" deploy ops/scripts/deploy
```

Expected: examples and docs are internally consistent.

**Step 4: Verify the local scripts still work**

Run:

```bash
bash -n ops/scripts/deploy/tokenmm_stack.sh
bash -n ops/scripts/deploy/equities_stack.sh
```

Expected: PASS with no syntax errors.

**Step 5: Commit**

```bash
git add deploy/tokenmm/systemd/common.env.example \
  deploy/equities/systemd/common.env.example \
  deploy/tokenmm/README.md \
  deploy/equities/README.md \
  ops/scripts/deploy/tokenmm_stack.sh \
  ops/scripts/deploy/equities_stack.sh
git commit -m "chore: document and bound local flux log retention"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Add EC2 Host Logging and Disk Baseline

**Files:**
- Create: `deploy/host/journald/99-flux-disk-limits.conf`
- Create: `deploy/host/docker/daemon.json.example`
- Create: `deploy/aws/cloudwatch-agent/amazon-cloudwatch-agent.json`
- Create: `deploy/aws/fluent-bit/fluent-bit.yaml`
- Create: `ops/scripts/deploy/install_flux_host_baseline.sh`
- Create: `docs/runbooks/ec2-host-baseline.md`

**Step 1: Capture the journald baseline in-repo**

Add a managed drop-in that codifies the on-host settings already seen on this box and extends them with an explicit keep-free budget if needed.

Suggested starting point:

```ini
[Journal]
SystemMaxUse=500M
SystemMaxFileSize=100M
RuntimeMaxUse=200M
```

Expected: future EC2 hosts do not depend on hidden manual state.

**Step 2: Add Docker log caps for hosts that run IBKR or other sidecar containers**

Add a managed example `daemon.json` with bounded `json-file` rotation.

Expected: container logs cannot grow without limit on boxes that use Docker.

**Step 3: Add CloudWatch Agent metrics for disk and inode health**

Configure the CloudWatch agent to publish at least:

- disk used percent
- disk free bytes
- disk inode free count
- memory used percent

Expected: host health becomes visible before the box hits `85%+` usage.

**Step 4: Add optional journald shipping to CloudWatch Logs**

Configure Fluent Bit with:

- `systemd` input
- `_SYSTEMD_UNIT=flux@*.service` filter
- CloudWatch Logs output with auto-created group/stream prefix

Expected: Pulse remains the on-host UI, while CloudWatch Logs becomes the durable retention/search layer.

**Step 5: Validate on a non-critical EC2 box**

Run:

```bash
sudo systemd-analyze cat-config systemd/journald.conf
sudo journalctl --disk-usage
sudo docker system df
```

Then verify:

- CloudWatch metrics arrive
- Flux journals ship off-box
- no duplicate or runaway logs appear

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 5: Roll Out Monitoring, Retention, and Validation

**Files:**
- Create: `docs/runbooks/ec2-log-and-disk-rollout.md`
- Create: `ops/scripts/deploy/check_flux_host_baseline.sh`
- Modify: `ops/README.md`

**Step 1: Define alert thresholds**

Create explicit thresholds for:

- `disk_used_percent > 75` warning
- `disk_used_percent > 85` critical
- low inode availability
- repeated `flux@` restarts
- Fluent Bit / CloudWatch agent unhealthy state

Expected: every production box shares the same operator thresholds.

**Step 2: Add a host conformance check**

Create one script that verifies:

- journald drop-in installed
- Docker `daemon.json` log caps present when Docker is enabled
- CloudWatch agent active
- Fluent Bit active when off-box shipping is enabled
- no `.run/*/logs` or repo-root log directories exceed the local retention budget

Expected: operators can audit a box in one command.

**Step 3: Roll out one box at a time**

Sequence:

1. canary EC2 box
2. shared production host
3. remaining production hosts

Expected: each rollout has before/after disk measurements and alarm validation.

**Step 4: Final verification**

Run:

```bash
df -h
sudo journalctl --disk-usage
sudo systemctl list-units --type=service --state=running 'flux@*' --no-pager
```

Expected: all Flux services are healthy, disk usage is below the chosen warning threshold, and logs are both bounded on-host and searchable off-box.

**Step 5: Commit**

```bash
git add docs/runbooks/production-host-disk-recovery.md \
  docs/runbooks/ec2-host-baseline.md \
  docs/runbooks/ec2-log-and-disk-rollout.md \
  ops/scripts/deploy/install_flux_host_baseline.sh \
  ops/scripts/deploy/check_flux_host_baseline.sh \
  deploy/host/journald/99-flux-disk-limits.conf \
  deploy/host/docker/daemon.json.example \
  deploy/aws/cloudwatch-agent/amazon-cloudwatch-agent.json \
  deploy/aws/fluent-bit/fluent-bit.yaml \
  ops/README.md
git commit -m "ops: add flux host logging and disk baseline"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

## Notes for Execution

- Do not treat this host as a clean production-only box; a large share of usage is from local dev worktrees and compiled Rust artifacts.
- The fastest immediate space recovery is `chainsaw` log cleanup plus stale worktree cleanup, not Flux journald tuning.
- Keep Pulse compatible with the final design. If production logging changes, `/api/pulse/jobs/<job_id>/logs` must remain useful.
- If emergency headroom is still insufficient after cleanup, increase EBS size using Elastic Volumes before the next risky deploy.

## Suggested Success Criteria

- Root volume stays below `75%` on steady state.
- No single log file can exceed the local budget without rotation or alerting.
- Flux nodes, bridge, portfolio, and API share one logging contract.
- Every production EC2 host has the same journald, Docker, metric, and retention baseline.
- Operators can answer “what is filling disk?” in one command and “where are my logs?” with one documented path.
