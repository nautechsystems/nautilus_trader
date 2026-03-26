# AWS-Managed Production Boxes Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Make the TokenMM and future production boxes self-managing and cheap by automating S3-first telemetry export, short log retention, host baselines, and cutover so live trading continues without manual AWS or host configuration.

**Architecture:** Keep the trading hot path local. Export the must-keep operational history to Parquet in S3 and query it through Athena. Keep raw quote-cycle data and logs only for short bounded windows by default. Defer always-on RDS from the current rollout.

**Tech Stack:** Python, SQLite WAL, Parquet, S3, Athena, AWS Secrets Manager, AWS Systems Manager, CloudWatch Logs, CloudWatch Agent, Fluent Bit, systemd, Bash deploy scripts, pytest.

**Context Docs:**
- Design: `docs/plans/2026-03-26-aws-managed-prod-boxes-design.md`
- PRD: `none`
- Relevant specs/runbooks: `docs/plans/2026-03-07-portfolio-snapshots-history.md`, `docs/plans/2026-03-17-tokenmm-telemetry-prod-autopilot.md`, `deploy/tokenmm/README.md`, `deploy/tokenmm/TELEMETRY_RDS_RUNBOOK.md`, `docs/runbooks/ec2-host-baseline.md`, `docs/runbooks/deploy-lanes.md`

**Decision Summary:**
- Trading correctness must never depend on AWS availability after startup.
- Durable history goes to S3 and Athena first; raw quote-cycle history and logs get short default retention; RDS is not part of the current required rollout.
- The PR must automate provisioning, retention, lifecycle policies, host baselines, and cutover so the operator does not hand-configure AWS resources or host files.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | in_progress | main | none | `docs/plans/2026-03-26-aws-managed-prod-boxes-design.md`, `docs/plans/2026-03-26-aws-managed-prod-boxes.md`, `deploy/tokenmm/*`, `deploy/aws/*`, `deploy/host/*`, `nautilus_trader/persistence/shipper/*`, `ops/scripts/deploy/*`, `tests/unit_tests/ops/deploy/*`, `tests/unit_tests/persistence/*`, `tests/unit_tests/examples/strategies/*`, `docs/runbooks/*` | `codex/lean-aws-managed-prod-boxes` | `/home/ubuntu/nautilus_trader/.worktrees/lean-aws-managed-prod-boxes` | none | `uv run --active --no-sync pytest -q tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py PASS (56 passed)` | Worktree bootstrapped; Task 1 verified; preparing commit and Task 2 |
| Task 1: Lock the lean managed-prod-box contract in docs and tests | completed | main | none | `docs/plans/2026-03-26-aws-managed-prod-boxes-design.md`, `docs/plans/2026-03-26-aws-managed-prod-boxes.md`, `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`, `tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py` | `codex/lean-aws-managed-prod-boxes` | `/home/ubuntu/nautilus_trader/.worktrees/lean-aws-managed-prod-boxes` | none | `uv run --active --no-sync pytest -q tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py PASS (56 passed)` | Lean architecture docs synced into branch and contract assertions added |
| Task 2: Extend telemetry config for data classes and retention policies | not_started | unassigned | Task 1: Lock the lean managed-prod-box contract in docs and tests | `nautilus_trader/persistence/shipper/config.py`, `nautilus_trader/persistence/shipper/run.py`, `nautilus_trader/persistence/shipper/service.py`, `deploy/tokenmm/tokenmm.live.toml`, `tests/unit_tests/persistence/test_telemetry_shipper.py`, `tests/unit_tests/persistence/test_telemetry_archive.py` | `shared` | `shared` | none | not_run | Plan revised |
| Task 3: Export must-keep datasets to S3 and Athena and keep only short raw quote-cycle history | not_started | unassigned | Task 2: Extend telemetry config for data classes and retention policies | `nautilus_trader/persistence/shipper/s3_archive.py`, `nautilus_trader/persistence/shipper/quote_cycle_archive.py`, `ops/scripts/deploy/tokenmm_telemetry_cutover.py`, `deploy/tokenmm/README.md`, `deploy/tokenmm/TELEMETRY_RDS_RUNBOOK.md`, `tests/unit_tests/persistence/test_telemetry_archive.py`, `tests/unit_tests/persistence/test_quote_cycle_archive.py`, `tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py` | `shared` | `shared` | none | not_run | Plan revised |
| Task 4: Finish the AWS host baseline and enforce short log retention | not_started | unassigned | Task 1: Lock the lean managed-prod-box contract in docs and tests | `ops/scripts/deploy/install_flux_host_baseline.sh`, `ops/scripts/deploy/check_flux_host_baseline.sh`, `deploy/host/journald/99-flux-disk-limits.conf`, `deploy/aws/fluent-bit/fluent-bit.yaml`, `deploy/aws/cloudwatch-agent/amazon-cloudwatch-agent.json`, `docs/runbooks/ec2-host-baseline.md`, `tests/unit_tests/ops/deploy/test_flux_host_baseline_contract.py` | `shared` | `shared` | none | not_run | Plan revised |
| Task 5: Make deploy entrypoints bootstrap S3, Athena, CloudWatch, and retention automatically | not_started | unassigned | Task 3: Export must-keep datasets to S3 and Athena and keep only short raw quote-cycle history, Task 4: Finish the AWS host baseline and enforce short log retention | `ops/scripts/deploy/bootstrap_tokenmm_telemetry_archive.sh`, `ops/scripts/deploy/tokenmm_stack.sh`, `ops/scripts/deploy/equities_stack.sh`, `ops/scripts/deploy/shared_strategy_stack.sh`, `ops/scripts/deploy/create_release_root.sh`, `deploy/tokenmm/systemd/tokenmm-telemetry-export.env.example`, `deploy/tokenmm/README.md`, `deploy/equities/README.md`, `docs/runbooks/deploy-lanes.md`, `tests/unit_tests/ops/deploy/test_shared_strategy_stack.py`, `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`, `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py` | `shared` | `shared` | none | not_run | Plan revised |
| Task 6: Update operator runbooks and perform guarded rollout verification | not_started | unassigned | Task 3: Export must-keep datasets to S3 and Athena and keep only short raw quote-cycle history, Task 4: Finish the AWS host baseline and enforce short log retention, Task 5: Make deploy entrypoints bootstrap S3, Athena, CloudWatch, and retention automatically | `deploy/tokenmm/README.md`, `deploy/tokenmm/TELEMETRY_RDS_RUNBOOK.md`, `docs/runbooks/ec2-host-baseline.md`, `docs/runbooks/deploy-lanes.md`, `docs/runbooks/aws-managed-prod-box-ops.md` | `shared` | `shared` | none | not_run | Plan revised |

---

### Task 1: Lock the lean managed-prod-box contract in docs and tests

**Files:**
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`
- Modify: `tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py`
- Modify: `docs/plans/2026-03-26-aws-managed-prod-boxes-design.md`
- Modify: `docs/plans/2026-03-26-aws-managed-prod-boxes.md`

**Dependencies:** `none`

**Write Scope:** `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`, `tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py`, `docs/plans/2026-03-26-aws-managed-prod-boxes-design.md`, `docs/plans/2026-03-26-aws-managed-prod-boxes.md`

**Verification Commands:**
- `pytest -q tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py`

**Step 1: Write the failing contract assertions**

Add contract coverage that requires:

- no manual AWS console configuration in the supported operator flow
- no synchronous AWS writes in strategy or risk paths
- durable history exported to `S3` and queryable through `Athena`
- short default retention for logs and raw quote-cycle history
- `RDS` to be optional and deferred rather than part of the required current rollout

Example assertion shape:

```python
assert "S3" in design_doc and "Athena" in design_doc
assert "short log retention" in design_doc
assert "RDS" in design_doc and "deferred" in design_doc
assert "no manual AWS console steps" in design_doc
```

**Step 2: Run the contract tests to confirm the gap**

Run: `pytest -q tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py`

Expected: FAIL until the docs and deploy contract references reflect the lean architecture.

**Step 3: Write the revised design and plan docs**

Document:

- retention by data importance
- S3 and Athena as the default durable history surface
- short log and raw quote-cycle retention
- deferred RDS criteria
- exact implementation tasks

**Step 4: Re-run the contract tests**

Run the same pytest command.

Expected: PASS

**Step 5: Commit**

```bash
git add \
  docs/plans/2026-03-26-aws-managed-prod-boxes-design.md \
  docs/plans/2026-03-26-aws-managed-prod-boxes.md \
  tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py \
  tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py
git commit -m "docs: define lean aws-managed prod box architecture"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Extend telemetry config for data classes and retention policies

**Files:**
- Modify: `nautilus_trader/persistence/shipper/config.py`
- Modify: `nautilus_trader/persistence/shipper/run.py`
- Modify: `nautilus_trader/persistence/shipper/service.py`
- Modify: `deploy/tokenmm/tokenmm.live.toml`
- Modify: `tests/unit_tests/persistence/test_telemetry_shipper.py`
- Create: `tests/unit_tests/persistence/test_telemetry_archive.py`

**Dependencies:** `Task 1: Lock the lean managed-prod-box contract in docs and tests`

**Write Scope:** `nautilus_trader/persistence/shipper/config.py`, `nautilus_trader/persistence/shipper/run.py`, `nautilus_trader/persistence/shipper/service.py`, `deploy/tokenmm/tokenmm.live.toml`, `tests/unit_tests/persistence/test_telemetry_shipper.py`, `tests/unit_tests/persistence/test_telemetry_archive.py`

**Verification Commands:**
- `pytest -q tests/unit_tests/persistence/test_telemetry_shipper.py tests/unit_tests/persistence/test_telemetry_archive.py`

**Step 1: Write the failing config and orchestration tests**

Require a config model that distinguishes:

- must-keep datasets
- short-lived raw quote-cycle history
- local spool caps
- S3 and Athena export settings
- short default retention windows

Example config target:

```toml
[telemetry_export]
enabled = true
durable_sink = "s3_athena"
raw_quote_cycles_enabled = true
raw_quote_cycle_local_hours = 48
raw_quote_cycle_s3_days = 7
core_history_s3_days = 365
structured_local_cap_gb = 8
quote_cycle_local_cap_gb = 12
```

**Step 2: Run the focused tests to confirm failure**

Run: `pytest -q tests/unit_tests/persistence/test_telemetry_shipper.py tests/unit_tests/persistence/test_telemetry_archive.py`

Expected: FAIL because the current shipper models a simpler retention shape.

**Step 3: Implement the minimal config and runtime split**

Add:

- explicit dataset classification by retention importance
- local cap settings for durable versus short-lived telemetry
- startup validation that rejects invalid retention or sink combinations
- runtime separation so must-keep history and raw quote-cycle handling can prune independently

**Step 4: Re-run the focused tests**

Run the same pytest command.

Expected: PASS

**Step 5: Commit**

```bash
git add \
  nautilus_trader/persistence/shipper/config.py \
  nautilus_trader/persistence/shipper/run.py \
  nautilus_trader/persistence/shipper/service.py \
  deploy/tokenmm/tokenmm.live.toml \
  tests/unit_tests/persistence/test_telemetry_shipper.py \
  tests/unit_tests/persistence/test_telemetry_archive.py
git commit -m "feat: classify telemetry by retention tier"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Export must-keep datasets to S3 and Athena and keep only short raw quote-cycle history

**Files:**
- Create: `nautilus_trader/persistence/shipper/s3_archive.py`
- Create: `nautilus_trader/persistence/shipper/quote_cycle_archive.py`
- Modify: `ops/scripts/deploy/tokenmm_telemetry_cutover.py`
- Modify: `deploy/tokenmm/README.md`
- Modify: `deploy/tokenmm/TELEMETRY_RDS_RUNBOOK.md`
- Modify: `tests/unit_tests/persistence/test_telemetry_archive.py`
- Modify: `tests/unit_tests/persistence/test_quote_cycle_archive.py`
- Modify: `tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py`

**Dependencies:** `Task 2: Extend telemetry config for data classes and retention policies`

**Write Scope:** `nautilus_trader/persistence/shipper/s3_archive.py`, `nautilus_trader/persistence/shipper/quote_cycle_archive.py`, `ops/scripts/deploy/tokenmm_telemetry_cutover.py`, `deploy/tokenmm/README.md`, `deploy/tokenmm/TELEMETRY_RDS_RUNBOOK.md`, `tests/unit_tests/persistence/test_telemetry_archive.py`, `tests/unit_tests/persistence/test_quote_cycle_archive.py`, `tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py`

**Verification Commands:**
- `pytest -q tests/unit_tests/persistence/test_telemetry_archive.py tests/unit_tests/persistence/test_quote_cycle_archive.py tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py`

**Step 1: Write the failing archive tests**

Require:

- must-keep datasets export to Parquet with stable partitioning by profile, dataset, and date
- Athena DDL or partition maintenance is deterministic
- raw quote-cycle segments are exported and pruned according to the short retention policy
- summary datasets survive far longer than the raw quote-cycle archive

Example behavioral expectation:

```python
result = export_dataset_batch(...)
assert result.s3_keys
assert result.athena_table == "tokenmm_orders"
assert result.local_batch_deleted is True
```

**Step 2: Run the focused tests to confirm failure**

Run: `pytest -q tests/unit_tests/persistence/test_telemetry_archive.py tests/unit_tests/persistence/test_quote_cycle_archive.py tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py`

Expected: FAIL because the current code does not implement the S3 and Athena export contract.

**Step 3: Implement archive and pruning behavior**

Implement:

- Parquet export for must-keep datasets
- Athena database, workgroup, and external table maintenance
- Parquet export for rotated raw quote-cycle segments
- lifecycle-friendly S3 layout
- local pruning only after successful export
- derived quote-cycle summaries that retain more cheaply than full raw events

Do not change the strategy hot path to write directly to S3 or Athena.

**Step 4: Re-run the focused tests**

Run the same pytest command.

Expected: PASS

**Step 5: Commit**

```bash
git add \
  nautilus_trader/persistence/shipper/s3_archive.py \
  nautilus_trader/persistence/shipper/quote_cycle_archive.py \
  ops/scripts/deploy/tokenmm_telemetry_cutover.py \
  deploy/tokenmm/README.md \
  deploy/tokenmm/TELEMETRY_RDS_RUNBOOK.md \
  tests/unit_tests/persistence/test_telemetry_archive.py \
  tests/unit_tests/persistence/test_quote_cycle_archive.py \
  tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py
git commit -m "feat: export telemetry to s3 and athena"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Finish the AWS host baseline and enforce short log retention

**Files:**
- Modify: `ops/scripts/deploy/install_flux_host_baseline.sh`
- Modify: `ops/scripts/deploy/check_flux_host_baseline.sh`
- Modify: `deploy/host/journald/99-flux-disk-limits.conf`
- Modify: `deploy/aws/fluent-bit/fluent-bit.yaml`
- Modify: `deploy/aws/cloudwatch-agent/amazon-cloudwatch-agent.json`
- Modify: `docs/runbooks/ec2-host-baseline.md`
- Create: `tests/unit_tests/ops/deploy/test_flux_host_baseline_contract.py`

**Dependencies:** `Task 1: Lock the lean managed-prod-box contract in docs and tests`

**Write Scope:** `ops/scripts/deploy/install_flux_host_baseline.sh`, `ops/scripts/deploy/check_flux_host_baseline.sh`, `deploy/host/journald/99-flux-disk-limits.conf`, `deploy/aws/fluent-bit/fluent-bit.yaml`, `deploy/aws/cloudwatch-agent/amazon-cloudwatch-agent.json`, `docs/runbooks/ec2-host-baseline.md`, `tests/unit_tests/ops/deploy/test_flux_host_baseline_contract.py`

**Verification Commands:**
- `bash -n ops/scripts/deploy/install_flux_host_baseline.sh ops/scripts/deploy/check_flux_host_baseline.sh`
- `pytest -q tests/unit_tests/ops/deploy/test_flux_host_baseline_contract.py`

**Step 1: Write the failing host-baseline tests**

Require:

- `ForwardToSyslog=no` in the repo-managed journald config
- CloudWatch Agent installation and validation
- Fluent Bit installation and validation
- `rsyslog` disablement or equivalent fanout removal
- default CloudWatch retention of `7 days`
- noisy access or request logs filtered, sampled, or disabled by default

Example config target:

```ini
[Journal]
SystemMaxUse=500M
SystemMaxFileSize=100M
RuntimeMaxUse=200M
SystemKeepFree=5G
ForwardToSyslog=no
```

**Step 2: Run the focused tests to confirm failure**

Run the commands above.

Expected: FAIL until the repo baseline explicitly disables duplicate syslog retention and encodes short centralized retention.

**Step 3: Implement the baseline corrections**

Install and validate:

- CloudWatch Agent
- Fluent Bit
- journald forwarding override
- rsyslog disablement or equivalent persistent fanout removal
- short CloudWatch Logs retention and log-source filtering

**Step 4: Re-run the focused tests**

Run the commands above.

Expected: PASS

**Step 5: Commit**

```bash
git add \
  ops/scripts/deploy/install_flux_host_baseline.sh \
  ops/scripts/deploy/check_flux_host_baseline.sh \
  deploy/host/journald/99-flux-disk-limits.conf \
  deploy/aws/fluent-bit/fluent-bit.yaml \
  deploy/aws/cloudwatch-agent/amazon-cloudwatch-agent.json \
  docs/runbooks/ec2-host-baseline.md \
  tests/unit_tests/ops/deploy/test_flux_host_baseline_contract.py
git commit -m "feat: enforce lean host logging baseline"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 5: Make deploy entrypoints bootstrap S3, Athena, CloudWatch, and retention automatically

**Files:**
- Create: `ops/scripts/deploy/bootstrap_tokenmm_telemetry_archive.sh`
- Modify: `ops/scripts/deploy/tokenmm_stack.sh`
- Modify: `ops/scripts/deploy/equities_stack.sh`
- Modify: `ops/scripts/deploy/shared_strategy_stack.sh`
- Modify: `ops/scripts/deploy/create_release_root.sh`
- Create: `deploy/tokenmm/systemd/tokenmm-telemetry-export.env.example`
- Modify: `deploy/tokenmm/README.md`
- Modify: `deploy/equities/README.md`
- Modify: `docs/runbooks/deploy-lanes.md`
- Modify: `tests/unit_tests/ops/deploy/test_shared_strategy_stack.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`

**Dependencies:** `Task 3: Export must-keep datasets to S3 and Athena and keep only short raw quote-cycle history`, `Task 4: Finish the AWS host baseline and enforce short log retention`

**Write Scope:** `ops/scripts/deploy/bootstrap_tokenmm_telemetry_archive.sh`, `ops/scripts/deploy/tokenmm_stack.sh`, `ops/scripts/deploy/equities_stack.sh`, `ops/scripts/deploy/shared_strategy_stack.sh`, `ops/scripts/deploy/create_release_root.sh`, `deploy/tokenmm/systemd/tokenmm-telemetry-export.env.example`, `deploy/tokenmm/README.md`, `deploy/equities/README.md`, `docs/runbooks/deploy-lanes.md`, `tests/unit_tests/ops/deploy/test_shared_strategy_stack.py`, `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`, `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`

**Verification Commands:**
- `bash -n ops/scripts/deploy/bootstrap_tokenmm_telemetry_archive.sh ops/scripts/deploy/tokenmm_stack.sh ops/scripts/deploy/equities_stack.sh ops/scripts/deploy/shared_strategy_stack.sh ops/scripts/deploy/create_release_root.sh`
- `pytest -q tests/unit_tests/ops/deploy/test_shared_strategy_stack.py tests/unit_tests/examples/strategies/test_equities_stack_contract.py tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`

**Step 1: Write the failing deploy-entrypoint tests**

Require the supported deploy flow to:

- apply the host baseline
- bootstrap or reuse S3, Athena, CloudWatch, secrets, and retention policies
- render env files from managed secrets and parameters
- provision or reuse release roots
- prune old release roots
- cut over live services without manual follow-up edits

**Step 2: Run the focused tests to confirm failure**

Run the commands above.

Expected: FAIL until the stack entrypoints call the archive bootstrap and retention helpers directly.

**Step 3: Implement the unified deploy flow**

Make the stack scripts the supported operator surface so that:

- one deploy or rollout command performs the required setup
- future boxes follow the same bootstrap path
- production boxes remain pinned to immutable release roots
- the current rollout does not require RDS

**Step 4: Re-run the focused tests**

Run the commands above.

Expected: PASS

**Step 5: Commit**

```bash
git add \
  ops/scripts/deploy/bootstrap_tokenmm_telemetry_archive.sh \
  ops/scripts/deploy/tokenmm_stack.sh \
  ops/scripts/deploy/equities_stack.sh \
  ops/scripts/deploy/shared_strategy_stack.sh \
  ops/scripts/deploy/create_release_root.sh \
  deploy/tokenmm/systemd/tokenmm-telemetry-export.env.example \
  deploy/tokenmm/README.md \
  deploy/equities/README.md \
  docs/runbooks/deploy-lanes.md \
  tests/unit_tests/ops/deploy/test_shared_strategy_stack.py \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py
git commit -m "feat: automate lean prod box deploy and cutover"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 6: Update operator runbooks and perform guarded rollout verification

**Files:**
- Modify: `deploy/tokenmm/README.md`
- Modify: `deploy/tokenmm/TELEMETRY_RDS_RUNBOOK.md`
- Modify: `docs/runbooks/ec2-host-baseline.md`
- Modify: `docs/runbooks/deploy-lanes.md`
- Create: `docs/runbooks/aws-managed-prod-box-ops.md`

**Dependencies:** `Task 3: Export must-keep datasets to S3 and Athena and keep only short raw quote-cycle history`, `Task 4: Finish the AWS host baseline and enforce short log retention`, `Task 5: Make deploy entrypoints bootstrap S3, Athena, CloudWatch, and retention automatically`

**Write Scope:** `deploy/tokenmm/README.md`, `deploy/tokenmm/TELEMETRY_RDS_RUNBOOK.md`, `docs/runbooks/ec2-host-baseline.md`, `docs/runbooks/deploy-lanes.md`, `docs/runbooks/aws-managed-prod-box-ops.md`

**Verification Commands:**
- `rg -n "S3|Athena|CloudWatch|Secrets Manager|SSM|7 days|raw quote-cycle|RDS.*deferred|current \\+ previous" deploy/tokenmm/README.md deploy/tokenmm/TELEMETRY_RDS_RUNBOOK.md docs/runbooks/ec2-host-baseline.md docs/runbooks/deploy-lanes.md docs/runbooks/aws-managed-prod-box-ops.md`
- `sudo systemctl status flux@tokenmm-telemetry-shipper.service --no-pager`
- `sudo journalctl --disk-usage`
- `curl -fsS http://127.0.0.1:5022/api/pulse/jobs | jq '.groups // .data // .'`

**Step 1: Write the operator runbook updates**

Document exactly:

- what the operator runs
- what the automation creates or reuses
- which datasets are kept long-term
- which datasets are kept only briefly
- how to query Athena
- what healthy log and disk usage look like after cutover
- when to temporarily increase raw quote-cycle retention
- when RDS would become a justified future upgrade

**Step 2: Run the documentation grep checks**

Run the first command above.

Expected: FAIL until the lean operator contract is documented consistently.

**Step 3: Perform the guarded rollout validation**

Validate on a real host:

- host baseline applied
- CloudWatch and Fluent Bit healthy
- must-keep telemetry exporting to S3 and queryable from Athena
- raw quote-cycle archive rotating and pruning on the short retention window
- local disk within target budget
- live trading unaffected

**Step 4: Re-run the documentation and live validation commands**

Run the commands above.

Expected: PASS for documentation checks and healthy runtime output on the rollout host.

**Step 5: Commit**

```bash
git add \
  deploy/tokenmm/README.md \
  deploy/tokenmm/TELEMETRY_RDS_RUNBOOK.md \
  docs/runbooks/ec2-host-baseline.md \
  docs/runbooks/deploy-lanes.md \
  docs/runbooks/aws-managed-prod-box-ops.md
git commit -m "docs: document lean managed prod box operations"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.
