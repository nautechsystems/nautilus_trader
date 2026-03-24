# TokenMM Live Hardening Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Eliminate the silent "process alive but TokenMM state/control-plane stale" failure mode, make restart recovery deterministic for stale cached venue state, and give operators one production health model that matches actual quoting safety.

**Architecture:** Treat this as two reliability projects plus one control-plane unification project. First, harden the Rust Redis msgbus publisher so a transient Redis/TLS failure does not silently consume buffered state or leave the node trading while Flux state stops advancing. Second, harden startup reconciliation so stale cached position artifacts either self-heal or fail with an explicit, operator-safe recovery path instead of ad hoc Redis surgery. Third, add a TokenMM readiness surface that combines process status, publish-path freshness, and strategy signal health so Pulse, Flux API, and operator automation all agree on whether a strategy is truly safe to run.

**Tech Stack:** Rust (`nautilus-infrastructure`, `nautilus-live`), Python (`flux` TokenMM runners, Pulse, ops scripts), Redis streams, pytest, cargo test, `uv` build/test workflow.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | completed | codex | none | `crates/infrastructure/src/redis/msgbus.rs`, `systems/flux/flux/runners/tokenmm/readiness.py`, `systems/flux/flux/runners/tokenmm/run_api.py`, `systems/flux/flux/pulse/api.py`, `ops/scripts/tokenmm_risk_audit.py`, `crates/live/src/manager.rs`, `nautilus_trader/live/execution_engine.py`, `docs/runbooks/tokenmm-risk-validation.md` | `plan/tokenmm-live-hardening-20260323` | `/home/ubuntu/nautilus_trader/.worktrees/tokenmm-live-hardening-20260323` | latest commit `39c6e184db` | `uv sync --active --all-groups --all-extras --inexact --no-install-package nautilus_trader` passed; `uv run --active --no-sync build.py` passed (`Build completed`, `Build time: 0:09:08.376716`); `cargo test -p nautilus-infrastructure msgbus -- --nocapture` passed twice (`16 passed`, then `17 passed` after the bounded-budget test landed); `cargo test -p nautilus-infrastructure redis -- --nocapture` passed (`31 passed`); `cargo test -p nautilus-common test_default_database_config -- --nocapture` passed; `uv run --active --no-sync pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_api.py tests/unit_tests/flux/pulse/test_api.py tests/unit_tests/examples/strategies/test_tokenmm_risk_audit_logic.py tests/unit_tests/flux/runners/test_tokenmm_readiness.py` passed (`53 passed`); `uv run --active --no-sync pytest -q tests/unit_tests/live/test_execution_recon.py tests/unit_tests/examples/strategies/test_tokenmm_run_node.py` passed twice (`171 passed`, then `172 passed` after the review regression landed); `uv run --active --no-sync pytest -q tests/unit_tests/examples/strategies/test_tokenmm_risk_audit_logic.py tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py` passed twice (`56 passed`); `CARGO_TARGET_DIR=/home/ubuntu/nautilus_trader/target cargo test -p nautilus-live manager -- --nocapture` passed twice after worktree-local `cargo clean` reclaimed disk space | Reviewers audited `7965d9fdea..WORKTREE`; the blocking follow-up fixes landed in `aad66e180a`, the final docs/ops slice landed in `39c6e184db`, and the branch is now ready for PR with the Pulse liveness-vs-readiness caveat documented in the runbook |
| Task 1: Bootstrap worktree and codify the msgbus failure contract | completed | codex | none | `crates/infrastructure/src/redis/msgbus.rs` | `plan/tokenmm-live-hardening-20260323` | `/home/ubuntu/nautilus_trader/.worktrees/tokenmm-live-hardening-20260323` | committed in `92511875b0` | `uv sync --active --all-groups --all-extras --inexact --no-install-package nautilus_trader` passed; `uv run --active --no-sync build.py` passed (`Build completed`, `Build time: 0:09:08.376716`); `cargo test -p nautilus-infrastructure msgbus -- --nocapture` failed in `test_drain_buffer_preserves_messages_when_pipeline_write_fails` and `test_publish_messages_retries_after_transient_redis_write_failure` | Added the three failure-contract tests plus the local production-rule comment; the terminal failure observability check is already green under current `is_closed()` semantics, but buffered writes are still dropped and transient write failures still kill the publisher |
| Task 2: Harden Redis publisher retry, buffering, and failure signaling | completed | codex | Task 1: Bootstrap worktree and codify the msgbus failure contract | `crates/infrastructure/src/redis/msgbus.rs`, `crates/infrastructure/src/redis/mod.rs`, `crates/common/src/msgbus/database.rs` | `plan/tokenmm-live-hardening-20260323` | `/home/ubuntu/nautilus_trader/.worktrees/tokenmm-live-hardening-20260323` | committed in `1d4a9b8c77` | `cargo test -p nautilus-infrastructure msgbus -- --nocapture` passed (`16 passed`); `cargo test -p nautilus-infrastructure redis -- --nocapture` passed (`31 passed`) | Implemented staged-buffer retention plus bounded reconnect/retry in `msgbus.rs`; no heartbeat/config wiring change was needed in this slice because terminal publisher exit already propagates through `is_closed()` and Task 3 readiness can key off stream freshness directly |
| Task 3: Add TokenMM readiness that reflects freshness, not just systemd liveness | completed | codex | Task 2: Harden Redis publisher retry, buffering, and failure signaling | `systems/flux/flux/runners/tokenmm/readiness.py`, `systems/flux/flux/runners/tokenmm/run_api.py`, `systems/flux/flux/runners/tokenmm/run_node.py`, `systems/flux/flux/pulse/api.py`, `ops/scripts/tokenmm_risk_audit.py`, `systems/flux/docs/api.md` | `plan/tokenmm-live-hardening-20260323` | `/home/ubuntu/nautilus_trader/.worktrees/tokenmm-live-hardening-20260323` | committed in `083e8db544` | `uv run --active --no-sync pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_api.py tests/unit_tests/flux/pulse/test_api.py tests/unit_tests/examples/strategies/test_tokenmm_risk_audit_logic.py tests/unit_tests/flux/runners/test_tokenmm_readiness.py` passed (`53 passed`) | Added a TokenMM-only readiness evaluator plus `/api/v1/readiness`, wired Pulse job snapshots and the risk-audit script to that single health model, and kept readiness stream-based instead of adding new node heartbeat config |
| Task 4: Make startup reconciliation recovery deterministic for stale cached positions | completed | codex | Task 3: Add TokenMM readiness that reflects freshness, not just systemd liveness | `systems/flux/flux/runners/tokenmm/run_node.py`, `crates/live/src/node.rs`, `crates/live/src/manager.rs`, `nautilus_trader/live/execution_engine.py`, `tests/unit_tests/live/test_execution_recon.py`, `crates/live/tests/manager.rs`, `docs/runbooks/tokenmm-risk-validation.md` | `plan/tokenmm-live-hardening-20260323` | `/home/ubuntu/nautilus_trader/.worktrees/tokenmm-live-hardening-20260323` | committed in `04d87dac9f` | `uv run --active --no-sync pytest -q tests/unit_tests/live/test_execution_recon.py -k stale_startup_position_after_missing_targeted_open_order_query` passed (`1 passed, 128 deselected`); `uv run --active --no-sync pytest -q tests/unit_tests/live/test_execution_recon.py tests/unit_tests/examples/strategies/test_tokenmm_run_node.py` passed (`171 passed in 1.30s`); `cargo clean` removed `12993 files, 10.0GiB total`; `CARGO_TARGET_DIR=/home/ubuntu/nautilus_trader/target cargo test -p nautilus-live manager -- --nocapture` passed (`tests/manager.rs`: `0 passed, 107 filtered`; `tests/node.rs`: `1 passed`) | Traced the active TokenMM startup path to the Python `LiveExecutionEngine`; the regression test had to run with a NETTING-mode client because startup only synthesizes a flat position report on that production path, and the finalized slice documents the bounded one-restart recovery path instead of ad hoc Redis cleanup |
| Task 5: Final rollout guardrails, docs, and verification bundle | completed | codex | Task 4: Make startup reconciliation recovery deterministic for stale cached positions | `docs/runbooks/tokenmm-risk-validation.md`, `docs/plans/2026-03-23-tokenmm-live-hardening.md`, `ops/scripts/tokenmm_risk_audit.py` | `plan/tokenmm-live-hardening-20260323` | `/home/ubuntu/nautilus_trader/.worktrees/tokenmm-live-hardening-20260323` | committed in `39c6e184db` | `uv run --active --no-sync pytest -q tests/unit_tests/examples/strategies/test_tokenmm_risk_audit_logic.py -k readiness_freshness_summary` passed (`1 passed, 5 deselected`); `uv run --active --no-sync pytest -q tests/unit_tests/live/test_execution_recon.py -k 'stale_startup_position_after_missing_targeted_open_order_query or targeted_query_returns_none_but_bulk_open_order_report_exists'` passed (`2 passed, 128 deselected`); `cargo test -p nautilus-infrastructure test_default_publish_retry_budget_is_bounded -- --nocapture` passed; `cargo test -p nautilus-common test_default_database_config -- --nocapture` passed; `uv run --active --no-sync pytest -q tests/unit_tests/examples/strategies/test_tokenmm_risk_audit_logic.py tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py` passed (`56 passed in 0.48s`); `uv run --active --no-sync pytest -q tests/unit_tests/live/test_execution_recon.py tests/unit_tests/examples/strategies/test_tokenmm_run_node.py` passed (`172 passed in 1.02s`); `cargo test -p nautilus-infrastructure msgbus -- --nocapture` passed (`17 passed`); `CARGO_TARGET_DIR=/home/ubuntu/nautilus_trader/target cargo test -p nautilus-live manager -- --nocapture` passed (`tests/node.rs`: `1 passed`) | Added explicit readiness/freshness evidence to the risk-audit success banner, promoted `/api/v1/readiness?profile=tokenmm` into the operator runbook, documented the Pulse liveness-vs-readiness caveat, and closed both review-blocking findings from the required `7965d9fdea..WORKTREE` audit before the final docs/ops commit |

---

### Task 1: Bootstrap worktree and codify the msgbus failure contract

**Files:**
- Modify: `crates/infrastructure/src/redis/msgbus.rs`
- Test: `crates/infrastructure/src/redis/msgbus.rs`

**Dependencies:** `none`

**Write Scope:** `crates/infrastructure/src/redis/msgbus.rs`

**Verification Commands:**
- `uv run --active --no-sync build.py`
- `cargo test -p nautilus-infrastructure msgbus -- --nocapture`

**Step 1: Bootstrap the fresh worktree**

Run: `uv run --active --no-sync build.py`
Expected: the worktree gains the built `nautilus_pyo3` surface so Python tests and import paths stop failing on `nautilus_trader.core.data`.

**Step 2: Write the failing msgbus tests first**

Add red tests in `crates/infrastructure/src/redis/msgbus.rs` for the observed contract gaps:
- a pipeline write failure must not consume buffered messages
- a transient Redis connection failure must be retried or reconnected without silently leaving the publisher dead
- a terminal publish failure must become observable to the rest of the process instead of only being logged

Suggested test names:
- `test_drain_buffer_preserves_messages_when_pipeline_write_fails`
- `test_publish_messages_retries_after_transient_redis_write_failure`
- `test_publish_task_exposes_terminal_failure_state`

**Step 3: Run the Rust msgbus tests to verify the red state**

Run: `cargo test -p nautilus-infrastructure msgbus -- --nocapture`
Expected: the new tests fail for the specific contract gaps above while the pre-existing msgbus tests still pass.

**Step 4: Record the intended failure contract in code comments right next to the tests**

Document, in brief comments near the new tests or helper code, the production rule: "tradeable node state must never silently stop publishing while the process keeps running." Keep the comment local and specific; do not add a design essay.

**Step 5: Commit the red-test slice**

```bash
git add crates/infrastructure/src/redis/msgbus.rs
git commit -m "test: codify tokenmm msgbus failure contract"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Harden Redis publisher retry, buffering, and failure signaling

**Files:**
- Modify: `crates/infrastructure/src/redis/msgbus.rs`
- Modify: `crates/infrastructure/src/redis/mod.rs`
- Modify: `crates/common/src/msgbus/database.rs`
- Test: `crates/infrastructure/src/redis/msgbus.rs`

**Dependencies:** `Task 1: Bootstrap worktree and codify the msgbus failure contract`

**Write Scope:** `crates/infrastructure/src/redis/msgbus.rs`, `crates/infrastructure/src/redis/mod.rs`, `crates/common/src/msgbus/database.rs`

**Verification Commands:**
- `cargo test -p nautilus-infrastructure msgbus -- --nocapture`
- `cargo test -p nautilus-infrastructure redis -- --nocapture`

**Step 1: Implement minimal buffering semantics that only remove messages after a successful Redis write**

Change the publish path so `drain_buffer` no longer uses `buffer.drain(..)` before `pipe.query_async(...)` succeeds. A simple `VecDeque` swap or staged batch clone is acceptable if it keeps the semantics obvious and bounded.

**Step 2: Implement transient reconnect behavior in the publish loop**

On write failure, reconnect using the existing Redis connection helper and retry the staged batch with bounded backoff. Do not let the publish task exit on the first transient failure if the node can still re-establish Redis safely.

**Step 3: Add an explicit terminal failure signal**

When the publisher exceeds the allowed retry budget or enters a non-recoverable state, surface that explicitly so the node can fail closed or at minimum publish an observable unhealthy signal. Logging alone is not sufficient.

**Step 4: Decide whether to enable msgbus heartbeat in TokenMM config as part of the same slice**

If `heartbeat_interval_secs` materially simplifies freshness detection, wire it through `MessageBusConfig` and `run_node.py`. If it adds noise without helping the control plane, document why and keep readiness based on state-stream advancement only. Do not add heartbeats "just because."

**Step 5: Run the Rust verification bundle and commit**

Run:
- `cargo test -p nautilus-infrastructure msgbus -- --nocapture`
- `cargo test -p nautilus-infrastructure redis -- --nocapture`

Then commit:

```bash
git add crates/infrastructure/src/redis/msgbus.rs crates/infrastructure/src/redis/mod.rs crates/common/src/msgbus/database.rs
git commit -m "fix: harden redis msgbus publisher for tokenmm"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Add TokenMM readiness that reflects freshness, not just systemd liveness

**Files:**
- Create: `systems/flux/flux/runners/tokenmm/readiness.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_api.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_node.py`
- Modify: `systems/flux/flux/pulse/api.py`
- Modify: `ops/scripts/tokenmm_risk_audit.py`
- Modify: `systems/flux/docs/api.md`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_run_api.py`
- Test: `tests/unit_tests/flux/pulse/test_api.py`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_risk_audit_logic.py`
- Test: `tests/unit_tests/flux/runners/test_tokenmm_readiness.py`

**Dependencies:** `Task 2: Harden Redis publisher retry, buffering, and failure signaling`

**Write Scope:** `systems/flux/flux/runners/tokenmm/readiness.py`, `systems/flux/flux/runners/tokenmm/run_api.py`, `systems/flux/flux/runners/tokenmm/run_node.py`, `systems/flux/flux/pulse/api.py`, `ops/scripts/tokenmm_risk_audit.py`, `systems/flux/docs/api.md`, `tests/unit_tests/examples/strategies/test_tokenmm_run_api.py`, `tests/unit_tests/flux/pulse/test_api.py`, `tests/unit_tests/examples/strategies/test_tokenmm_risk_audit_logic.py`, `tests/unit_tests/flux/runners/test_tokenmm_readiness.py`

**Verification Commands:**
- `uv run --active --no-sync pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_api.py tests/unit_tests/flux/pulse/test_api.py tests/unit_tests/examples/strategies/test_tokenmm_risk_audit_logic.py tests/unit_tests/flux/runners/test_tokenmm_readiness.py`

**Step 1: Copy the right pattern from Equities before inventing anything**

Read `systems/flux/flux/runners/equities/readiness.py` and extract only the reusable parts: threshold model, structured checks, JSON output shape, and failure summaries. Keep TokenMM-specific logic separate instead of forking Equities wholesale.

**Step 2: Write failing readiness tests**

Add red tests for these production rules:
- `systemd active` plus stale TokenMM state stream is not ready
- publisher failure / missing recent state update is not ready
- Pulse job snapshot should expose the freshness/readiness result, not just raw `systemctl` state
- `tokenmm_risk_audit.py` should fail closed on stale-state or publish-health failures

**Step 3: Implement TokenMM readiness**

Create `systems/flux/flux/runners/tokenmm/readiness.py` to compute readiness from:
- required strategy IDs
- recent `flux.makerv3.state` stream timestamps
- API `/signals` stale-state semantics
- any explicit publisher failure or heartbeat key introduced in Task 2

Wire the result into TokenMM `run_api.py`, and extend Pulse and risk-audit surfaces to include or consume that readiness result.

**Step 4: Keep one health model across operator surfaces**

Do not create separate definitions of "healthy" in Pulse, Flux API, and the risk audit script. Centralize the readiness decision or at least centralize its thresholds and shape.

**Step 5: Run the Python readiness bundle and commit**

Run: `uv run --active --no-sync pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_api.py tests/unit_tests/flux/pulse/test_api.py tests/unit_tests/examples/strategies/test_tokenmm_risk_audit_logic.py tests/unit_tests/flux/runners/test_tokenmm_readiness.py`

Then commit:

```bash
git add systems/flux/flux/runners/tokenmm/readiness.py systems/flux/flux/runners/tokenmm/run_api.py systems/flux/flux/runners/tokenmm/run_node.py systems/flux/flux/pulse/api.py ops/scripts/tokenmm_risk_audit.py systems/flux/docs/api.md tests/unit_tests/examples/strategies/test_tokenmm_run_api.py tests/unit_tests/flux/pulse/test_api.py tests/unit_tests/examples/strategies/test_tokenmm_risk_audit_logic.py tests/unit_tests/flux/runners/test_tokenmm_readiness.py
git commit -m "feat: add tokenmm readiness and stale publish detection"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Make startup reconciliation recovery deterministic for stale cached positions

**Files:**
- Modify: `systems/flux/flux/runners/tokenmm/run_node.py`
- Modify: `crates/live/src/node.rs`
- Modify: `crates/live/src/manager.rs`
- Modify: `nautilus_trader/live/execution_engine.py`
- Modify: `docs/runbooks/tokenmm-risk-validation.md`
- Test: `tests/unit_tests/live/test_execution_recon.py`
- Test: `crates/live/tests/manager.rs`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`

**Dependencies:** `Task 3: Add TokenMM readiness that reflects freshness, not just systemd liveness`

**Write Scope:** `systems/flux/flux/runners/tokenmm/run_node.py`, `crates/live/src/node.rs`, `crates/live/src/manager.rs`, `nautilus_trader/live/execution_engine.py`, `docs/runbooks/tokenmm-risk-validation.md`, `tests/unit_tests/live/test_execution_recon.py`, `crates/live/tests/manager.rs`, `tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`

**Verification Commands:**
- `cargo test -p nautilus-live manager -- --nocapture`
- `uv run --active --no-sync pytest -q tests/unit_tests/live/test_execution_recon.py tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`

**Step 1: Trace the real startup reconciliation path for TokenMM nodes**

Before editing anything, prove whether live TokenMM nodes execute startup reconciliation in the Rust live manager path, the Python execution engine path, or both. Capture that answer in the Progress Tracker notes. Do not patch both paths unless the trace proves both matter in production.

**Step 2: Write the failing recovery test on the active runtime path**

Add a red test for the exact incident shape: a stale cached position artifact remains locally after the venue no longer reports that position, and restart should either safely purge or explicitly quarantine the stale cache entry instead of forcing manual Redis deletion.

**Step 3: Implement deterministic cleanup or quarantine**

Implement the minimal safe behavior:
- if the artifact can be proven stale from venue truth, purge it automatically
- if it cannot be proven stale, fail closed with a specific operator-visible reason and a safe cleanup command or documented flow

Do not add permissive "best effort" cleanup that can hide a real position mismatch.

**Step 4: Update the TokenMM runbook with the exact recovery path**

The operator should not need to guess whether to restart, delete a Redis key, or leave the strategy down. Add the canonical sequence, checks, and failure conditions to `docs/runbooks/tokenmm-risk-validation.md`.

**Step 5: Run verification and commit**

Run:
- `cargo test -p nautilus-live manager -- --nocapture`
- `uv run --active --no-sync pytest -q tests/unit_tests/live/test_execution_recon.py tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`

Then commit:

```bash
git add systems/flux/flux/runners/tokenmm/run_node.py crates/live/src/node.rs crates/live/src/manager.rs nautilus_trader/live/execution_engine.py docs/runbooks/tokenmm-risk-validation.md tests/unit_tests/live/test_execution_recon.py crates/live/tests/manager.rs tests/unit_tests/examples/strategies/test_tokenmm_run_node.py
git commit -m "fix: harden tokenmm startup reconciliation recovery"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 5: Final rollout guardrails, docs, and verification bundle

**Files:**
- Modify: `docs/runbooks/tokenmm-risk-validation.md`
- Modify: `docs/plans/2026-03-23-tokenmm-live-hardening.md`
- Modify: `ops/scripts/tokenmm_risk_audit.py`

**Dependencies:** `Task 4: Make startup reconciliation recovery deterministic for stale cached positions`

**Write Scope:** `docs/runbooks/tokenmm-risk-validation.md`, `docs/plans/2026-03-23-tokenmm-live-hardening.md`, `ops/scripts/tokenmm_risk_audit.py`

**Verification Commands:**
- `uv run --active --no-sync pytest -q tests/unit_tests/examples/strategies/test_tokenmm_risk_audit_logic.py tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`
- `cargo test -p nautilus-infrastructure msgbus -- --nocapture`
- `cargo test -p nautilus-live manager -- --nocapture`

**Step 1: Add rollout notes and explicit SLO-facing checks**

Document the checks that prove the incident class is fixed:
- state stream freshness advances for all enabled TokenMM strategies
- readiness fails closed when publish freshness stops
- restart recovery for stale cached positions is deterministic and documented

**Step 2: Ask two review-oriented subagents to critique the implementation before calling it done**

Use review agents, not explorers:
- one `quality_reviewer` with `model: "gpt-5.4"` and `reasoning_effort: "xhigh"` to critique architecture, rollout risk, and hidden regressions
- one `spec_reviewer` or `default` with `model: "gpt-5.4"` and `reasoning_effort: "xhigh"` to verify the implementation actually addresses the March 23 Bybit/OKX stale-state incident modes

Give each reviewer the exact diff range and require file/line findings.

**Step 3: Run the final verification bundle**

Run:
- `uv run --active --no-sync pytest -q tests/unit_tests/examples/strategies/test_tokenmm_risk_audit_logic.py tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`
- `cargo test -p nautilus-infrastructure msgbus -- --nocapture`
- `cargo test -p nautilus-live manager -- --nocapture`

If a fuller relevant Python suite exists after code lands, run it too and record the exact command/result in the tracker.

**Step 4: Update this plan tracker with exact commits and verification results**

Do not leave the tracker stale. Fill in owners, statuses, commit SHAs, and last verification commands so a future operator can audit what happened without reading chat history.

**Step 5: Commit the final guardrail/doc slice**

```bash
git add docs/runbooks/tokenmm-risk-validation.md docs/plans/2026-03-23-tokenmm-live-hardening.md ops/scripts/tokenmm_risk_audit.py
git commit -m "docs: finalize tokenmm live hardening rollout plan"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.
