# TokenMM Telemetry Prod Autopilot Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Make TokenMM telemetry storage self-managing on the live trading host by provisioning a managed PostgreSQL sink, wiring the shipper to it securely, bounding local SQLite retention, and executing a production cutover that reclaims the current oversized local telemetry files.

**Architecture:** Keep the existing production write path unchanged on the hot trading path: TokenMM nodes and portfolio continue writing local SQLite first under `/var/lib/nautilus/telemetry/tokenmm`. Add managed PostgreSQL on AWS RDS as the long-retention sink, load its credentials from AWS Secrets Manager for the shipper, lower local retention so the box only carries a short spool window, and add a guarded cutover script that verifies shipping before rotating the live local telemetry files. Fail closed when telemetry shipping is misconfigured so the service does not crash-loop silently.

**Tech Stack:** Python 3.12/3.13, uv, pytest, AWS CLI, Amazon RDS for PostgreSQL, AWS Secrets Manager, systemd, SQLite WAL, psycopg/PostgreSQL.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | completed | main | Prod deployed from stable root, branch pushed, and PR opened at `clickconfirm/nautilus-trader#57` |
| Task 1: Lock The Managed Telemetry Contract In Tests | completed | main | `pytest --noconftest` contract slices passed in both worktree and stable root |
| Task 2: Add AWS Bootstrap Automation For The Telemetry Sink | completed | main | Provisioned `nautilus-tokenmm-telemetry` RDS PostgreSQL 16.13 in `ap-southeast-1`, plus SG, subnet group, and Secrets Manager secret |
| Task 3: Auto-Load Postgres Credentials And Harden Shipper Startup | completed | main | Shipper wrapper deployed; live `flux@tokenmm-telemetry-shipper.service` now runs cleanly with AWS-loaded credentials |
| Task 4: Bound Local Telemetry Retention And Add Health Guardrails | completed | main | `prune_retention_hours = 48`, health timer installed/started, healthcheck passes on prod thresholds |
| Task 5: Execute The Production Backfill And Disk-Reclaim Cutover | completed | main | Deleted oversized balance/portfolio snapshot SQLite surfaces and restarted node/portfolio services; `/` dropped to 71% used with 185G free |
| Task 6: Update Runbooks, Verify End-To-End, And Open The PR | completed | main | Branch pushed to both remotes and PR opened at `clickconfirm/nautilus-trader#57` |

---

## Assumptions To Carry Into Execution

1. AWS account `670513421539` in region `ap-southeast-1` has no existing RDS instances or clusters, so the implementation should provision a new telemetry sink rather than depend on an undiscovered shared database.
2. The live host is EC2 instance `i-0e9325adc56487b65` in default VPC `vpc-0693d6520a1610fa6`, subnet `subnet-055913a46385a79f6`, with security groups `sg-0ba7e1b212b340f71` and `sg-0ee02183c0f135883`.
3. The current TokenMM telemetry sink should be a dedicated managed PostgreSQL deployment in the same region, private to the VPC, with storage autoscaling and backups enabled.
4. Local SQLite on the trading box is a short-retention spool only after cutover; local retention should be reduced from `168` hours to `48` hours unless execution evidence justifies a different bound.
5. Production deployment is allowed from this branch after review-quality verification, with the live host updated as part of the execution wave.

## Baseline Notes

1. Current prod disk usage evidence:
   - `/var/lib/nautilus/telemetry/tokenmm` is approximately `204G`.
   - `balance_snapshots.sqlite` is approximately `177G`.
   - `quote_cycles.sqlite` is approximately `27G`.
2. Current config drift evidence:
   - `deploy/tokenmm/tokenmm.live.toml` enables the telemetry shipper with `prune_retention_hours = 168`.
   - `flux@tokenmm-telemetry-shipper.service` was crash-looping on missing `NAUTILUS_TELEMETRY_PG_*`.
   - `flux@.service` already has `RestartPreventExitStatus=78`, so execution should use exit code `78` for fail-closed misconfiguration paths.
3. Fresh worktree verification note:
   - `uv run --group test pytest -q tests/unit_tests/persistence/test_telemetry_shipper.py` in a fresh worktree triggers a full Rust/Cython build on cold start; expect first-run verification to be materially slower than warm runs.

### Task 1: Lock The Managed Telemetry Contract In Tests

**Files:**
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`
- Create: `tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`
- Test: `tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py`

**Step 1: Write failing deploy-contract assertions**

Add tests that require:
- a dedicated telemetry bootstrap script under `ops/scripts/deploy/`
- a shipper wrapper that can load AWS-managed Postgres credentials
- `deploy/tokenmm/systemd/common.env.example` to declare the AWS region plus a telemetry Postgres secret-id path
- docs to describe the managed RDS + secret bootstrap flow
- the live config to use a shorter local retention window

**Step 2: Run tests to verify they fail**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py \
  tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py
```

Expected:
- FAIL on missing bootstrap/wrapper script references
- FAIL on missing env/example/doc assertions

**Step 3: Keep task scope contract-only**

Do not implement AWS calls or deploy logic yet. This task only freezes the intended operator surface and file-level contract.

**Step 4: Re-run the targeted contract tests after implementation work lands in later tasks**

Run the same pytest command after Tasks 2-4.

Expected:
- PASS with the new deploy/runtime contract in place

**Step 5: Commit**

```bash
git add \
  tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py \
  tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py
git commit -m "test: lock tokenmm telemetry autopilot contract"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Add AWS Bootstrap Automation For The Telemetry Sink

**Files:**
- Create: `ops/scripts/deploy/bootstrap_tokenmm_telemetry_rds.sh`
- Create: `deploy/tokenmm/systemd/tokenmm-telemetry-rds.env.example`
- Modify: `deploy/tokenmm/README.md`
- Modify: `deploy/tokenmm/TELEMETRY_RDS_RUNBOOK.md`
- Test: `tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py`

**Step 1: Write the failing script-level test**

Require the bootstrap script to:
- discover the current EC2 instance/VPC/subnets from metadata or AWS CLI
- create or reuse a security group for PostgreSQL ingress from the host security groups only
- create or reuse an RDS subnet group spanning the VPC subnets
- create or reuse a PostgreSQL instance identifier dedicated to TokenMM telemetry
- generate and store credentials in AWS Secrets Manager
- emit a stable `.env` fragment with `NAUTILUS_TELEMETRY_PG_SECRET_ID`, region, host, db name, and schema

**Step 2: Run the new deploy test to verify it fails**

Run:

```bash
uv run --group test pytest -q tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py
```

Expected:
- FAIL on missing bootstrap script and template

**Step 3: Implement the bootstrap script**

Implement a bash script that defaults to:
- region `ap-southeast-1`
- engine `postgres`
- dedicated DB identifier such as `nautilus-tokenmm-telemetry`
- private-only access
- storage autoscaling and automated backups
- secret id under a stable path such as `/nautilus/tokenmm/telemetry/postgres`

The script must support:
- `--dry-run`
- `--apply-host-env` to write/update a host env fragment safely
- idempotent re-runs without replacing the DB unnecessarily

**Step 4: Document the operator entrypoint**

Update docs so the primary flow is:

```bash
sudo TOKENMM_DEPLOY_ROOT="${TOKENMM_DEPLOY_ROOT}" \
  ops/scripts/deploy/bootstrap_tokenmm_telemetry_rds.sh --apply-host-env
```

Expected:
- docs describe exactly one supported bootstrap path
- operator is not told to handcraft RDS config manually

**Step 5: Run the targeted test**

Run:

```bash
uv run --group test pytest -q tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py
```

Expected:
- PASS

**Step 6: Commit**

```bash
git add \
  ops/scripts/deploy/bootstrap_tokenmm_telemetry_rds.sh \
  deploy/tokenmm/systemd/tokenmm-telemetry-rds.env.example \
  deploy/tokenmm/README.md \
  deploy/tokenmm/TELEMETRY_RDS_RUNBOOK.md \
  tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py
git commit -m "feat: add tokenmm telemetry rds bootstrap automation"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Auto-Load Postgres Credentials And Harden Shipper Startup

**Files:**
- Create: `ops/scripts/deploy/run_tokenmm_telemetry_shipper.sh`
- Modify: `ops/scripts/deploy/install_tokenmm_systemd.sh`
- Modify: `deploy/tokenmm/systemd/common.env.example`
- Modify: `nautilus_trader/persistence/shipper/run.py`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`
- Modify: `tests/unit_tests/persistence/test_telemetry_shipper.py`

**Step 1: Write failing tests for the hardened startup path**

Require:
- the shipper env to run the wrapper script instead of raw `python3 -m ...`
- the wrapper to load Postgres credentials from AWS Secrets Manager when a secret id is configured
- the shipper to exit with status `78` on missing/invalid Postgres config so systemd does not restart-loop
- `common.env.example` to include `TOKENMM_AWS_REGION` and `NAUTILUS_TELEMETRY_PG_SECRET_ID`

**Step 2: Run the targeted tests to verify failure**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py \
  tests/unit_tests/persistence/test_telemetry_shipper.py -k 'telemetry or tokenmm'
```

Expected:
- FAIL on missing wrapper and fail-closed behavior

**Step 3: Implement the wrapper and fail-closed exit path**

The wrapper must:
- load the secret JSON from AWS Secrets Manager
- export `NAUTILUS_TELEMETRY_PG_*` vars for the Python process
- fail with exit `78` and a clear stderr message if the secret or required fields are missing

The Python entrypoint must:
- convert configuration/connection bootstrap failures into exit code `78`
- preserve normal nonzero exits for real runtime failures after startup

**Step 4: Re-run targeted tests**

Run the same pytest command from Step 2.

Expected:
- PASS

**Step 5: Commit**

```bash
git add \
  ops/scripts/deploy/run_tokenmm_telemetry_shipper.sh \
  ops/scripts/deploy/install_tokenmm_systemd.sh \
  deploy/tokenmm/systemd/common.env.example \
  nautilus_trader/persistence/shipper/run.py \
  tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py \
  tests/unit_tests/persistence/test_telemetry_shipper.py
git commit -m "feat: harden tokenmm telemetry shipper startup"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Bound Local Telemetry Retention And Add Health Guardrails

**Files:**
- Modify: `deploy/tokenmm/tokenmm.live.toml`
- Create: `ops/scripts/deploy/tokenmm_telemetry_healthcheck.py`
- Create: `deploy/tokenmm/systemd/flux-tokenmm-telemetry-health.service`
- Create: `deploy/tokenmm/systemd/flux-tokenmm-telemetry-health.timer`
- Modify: `ops/scripts/deploy/install_tokenmm_systemd.sh`
- Modify: `deploy/tokenmm/README.md`
- Modify: `deploy/tokenmm/TELEMETRY_RDS_RUNBOOK.md`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`
- Create: `tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py`

**Step 1: Freeze the retention and health-check expectations in tests**

Require:
- `prune_retention_hours = 48` in the live shared config
- a health-check script that reads local telemetry dir size, shipper state DB, and systemd unit state
- installed systemd timer/service artifacts for telemetry health

**Step 2: Run the failing test slice**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py \
  tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py -k 'telemetry'
```

Expected:
- FAIL on retention and missing health-check artifacts

**Step 3: Implement bounded spool behavior**

Implement:
- local retention lowered to `48` hours
- a health-check script with threshold flags such as:
  - `--max-telemetry-dir-gb`
  - `--max-root-usage-pct`
  - `--max-shipper-lag-minutes`
- systemd timer every 10 minutes
- clear journal output describing exactly which threshold failed

The health check should fail loudly; it must not delete live data behind the operator’s back.

**Step 4: Re-run the targeted tests**

Run the same pytest command from Step 2.

Expected:
- PASS

**Step 5: Commit**

```bash
git add \
  deploy/tokenmm/tokenmm.live.toml \
  ops/scripts/deploy/tokenmm_telemetry_healthcheck.py \
  deploy/tokenmm/systemd/flux-tokenmm-telemetry-health.service \
  deploy/tokenmm/systemd/flux-tokenmm-telemetry-health.timer \
  ops/scripts/deploy/install_tokenmm_systemd.sh \
  deploy/tokenmm/README.md \
  deploy/tokenmm/TELEMETRY_RDS_RUNBOOK.md \
  tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py \
  tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py
git commit -m "feat: bound tokenmm local telemetry retention"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 5: Execute The Production Backfill And Disk-Reclaim Cutover

**Files:**
- Create: `ops/scripts/deploy/tokenmm_telemetry_cutover.py`
- Modify: `deploy/tokenmm/TELEMETRY_RDS_RUNBOOK.md`
- Modify: `deploy/tokenmm/README.md`
- Test: `tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py`

**Step 1: Write a failing contract test for the cutover script**

Require the cutover script to:
- verify local SQLite sources exist
- verify the Postgres sink is reachable
- bootstrap the schema if missing
- confirm shipper cursor catch-up against local max rowids
- stop the TokenMM target for a short maintenance window
- rotate or delete local telemetry files only after successful ship verification
- restart the TokenMM target and verify new local DB activity

**Step 2: Run the failing test**

Run:

```bash
uv run --group test pytest -q tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py -k cutover
```

Expected:
- FAIL on missing cutover script contract

**Step 3: Implement the guarded cutover**

The script must support:
- `--dry-run`
- `--once` ship pass
- `--wait-for-catchup`
- `--delete-local-after-cutover`

The production execution sequence is:

```bash
sudo TOKENMM_DEPLOY_ROOT="${TOKENMM_DEPLOY_ROOT}" \
  ops/scripts/deploy/bootstrap_tokenmm_telemetry_rds.sh --apply-host-env
.venv/bin/python -m nautilus_trader.persistence.shipper.run \
  --config deploy/tokenmm/tokenmm.live.toml --bootstrap-postgres
sudo systemctl restart flux@tokenmm-telemetry-shipper.service
sudo .venv/bin/python ops/scripts/deploy/tokenmm_telemetry_cutover.py --wait-for-catchup --delete-local-after-cutover
```

Expected:
- local files are recreated fresh and small after restart
- disk usage drops materially on the live host

**Step 4: Re-run the targeted cutover contract test**

Run the same pytest command from Step 2.

Expected:
- PASS

**Step 5: Commit**

```bash
git add \
  ops/scripts/deploy/tokenmm_telemetry_cutover.py \
  deploy/tokenmm/TELEMETRY_RDS_RUNBOOK.md \
  deploy/tokenmm/README.md \
  tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py
git commit -m "feat: add tokenmm telemetry cutover automation"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 6: Update Runbooks, Verify End-To-End, And Open The PR

**Files:**
- Modify: `deploy/tokenmm/README.md`
- Modify: `deploy/tokenmm/TELEMETRY_RDS_RUNBOOK.md`
- Modify: `docs/fluxboard/tokenmm_runbook.md`
- Modify: `docs/flux/api.md`
- Modify: `docs/plans/2026-03-17-tokenmm-telemetry-prod-autopilot.md`

**Step 1: Run the targeted verification matrix**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/persistence/test_telemetry_shipper.py \
  tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py \
  tests/unit_tests/ops/deploy/test_tokenmm_telemetry_autopilot.py
```

Run:

```bash
python3 -m json.tool /home/ubuntu/nautilus_trader/.worktrees/tokenmm-telemetry-prod-ops-20260317/deploy/tokenmm/tokenmm.live.toml >/dev/null
```

Run:

```bash
git diff --check
```

Expected:
- all targeted tests PASS
- config/doc syntax checks PASS
- diff check clean

**Step 2: Execute the live rollout on the production host**

Use the stable deploy root, not the worktree:

```bash
export TOKENMM_DEPLOY_ROOT=/home/ubuntu/nautilus_trader
cd "${TOKENMM_DEPLOY_ROOT}"
make build
pnpm --dir fluxboard install --frozen-lockfile
pnpm --dir fluxboard build
pnpm --dir pulse-ui install --frozen-lockfile
pnpm --dir pulse-ui build
.venv/bin/python ops/scripts/deploy/tokenmm_rollout_preflight.py
sudo TOKENMM_DEPLOY_ROOT="${TOKENMM_DEPLOY_ROOT}" ops/scripts/deploy/install_tokenmm_systemd.sh
sudo TOKENMM_DEPLOY_ROOT="${TOKENMM_DEPLOY_ROOT}" ops/scripts/deploy/bootstrap_tokenmm_telemetry_rds.sh --apply-host-env
sudo systemctl daemon-reload
sudo systemctl restart flux@tokenmm-telemetry-shipper.service
sudo systemctl restart flux-tokenmm.target
sudo .venv/bin/python ops/scripts/deploy/tokenmm_telemetry_cutover.py --wait-for-catchup --delete-local-after-cutover
df -h /
sudo journalctl -u flux@tokenmm-telemetry-shipper.service -n 50 --no-pager
```

Expected:
- shipper active without restart loop
- TokenMM target healthy after restart
- root filesystem has comfortable free space
- local telemetry dir reduced to a short-retention spool

**Step 3: Capture deployment evidence**

Record in the PR description:
- AWS RDS identifier and secret id path
- post-cutover `df -h /`
- local telemetry dir size
- shipper status
- representative Postgres row counts

**Step 4: Open the PR**

```bash
git push -u origin codex/tokenmm-telemetry-prod-ops-20260317
gh pr create --fill
```

**Step 5: Commit docs/progress updates if needed**

```bash
git add \
  deploy/tokenmm/README.md \
  deploy/tokenmm/TELEMETRY_RDS_RUNBOOK.md \
  docs/fluxboard/tokenmm_runbook.md \
  docs/flux/api.md \
  docs/plans/2026-03-17-tokenmm-telemetry-prod-autopilot.md
git commit -m "docs: record tokenmm telemetry prod autopilot rollout"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.
