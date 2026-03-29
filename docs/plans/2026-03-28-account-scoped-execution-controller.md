# Account-Scoped Execution Controller Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Build only the first production package for controller-managed execution: a single-host shadow controller, one active `equities / ibkr.hedge.main` writer canary, and a one-bounce rollback path. Read-side authority, multi-box hardening, and TokenMM migration stay as follow-on approval packages.

**Architecture:** Keep `strategy_id` as the external lifecycle identity and use the existing Nautilus external-client seam as the first controller insertion point. Add a manually enumerated `controller_scope_id` contract, lock deterministic ownership/claim semantics up front, implement a synchronous ownership WAL plus asynchronous materializations, run controllers in shadow mode first, prove latency/failover/split-brain gates on a single-host canary, then activate the `equities / ibkr.hedge.main` writer domain. Only after that can separate approval packages move the read side or TokenMM Binance shared writer domains.

**Tech Stack:** Python, Cython-backed Nautilus execution surfaces, SQLite WAL, Redis, Flux shared runners, TOML deployment config, pytest, systemd deploy surfaces, Unix domain sockets for same-host controller transport.

**Context Docs:**
- Design: `docs/plans/2026-03-28-account-scoped-execution-controller-design.md`
- PRD: `none`
- Relevant specs/runbooks: `docs/concepts/live.md`, `docs/runbooks/deploy-lanes.md`, `docs/runbooks/tokenmm-risk-validation.md`, `docs/runbooks/equities-binance-perp-market-making.md`, `deploy/equities/README.md`, `deploy/tokenmm/README.md`

**Decision Summary:**
- The first execution package stops at one active `equities / ibkr.hedge.main` writer canary and one-bounce rollback.
- Public Nautilus issue history is treated as a caution signal only; implementation decisions still have to be proven against this repo's fork and runtime.
- Controller-managed lanes start at the external-client / adapter seam; Nautilus core patches are out of scope unless a specific invariant cannot be enforced there.
- Strategy/controller ownership is locked by an explicit state machine and ID chain before transport or canary work begins.
- V1 manually enumerates `controller_scope_id` values; no automatic writer-domain derivation beyond validation helpers.
- The first active writer canary is `equities / ibkr.hedge.main`.
- Active cutover remains single-host and no-standby until replicated ownership logging and split-brain drills are complete.
- Venue writes are not considered owned until the controller appends a synchronous ownership WAL record.
- Any future controller-owned read-side switch requires controller-sequenced freshness markers on snapshots.

## Package Boundary

**Current execution package:** Tasks 0-8 only.

This package ends when all of the following are true:

- shadow controller path is running in production shape
- one active `equities / ibkr.hedge.main` controller-owned writer canary is live
- rollback to legacy ownership works in one bounce

**Explicitly out of this package:** Tasks 9-11.

Those tasks are follow-on approval packages for:

- read-side authority switch
- multi-box durability and stale-writer hardening
- TokenMM shared Binance migration

After Task 8, Task 9 and Task 10 may be approved independently. Task 11 stays
blocked on Task 10.

**Internal cut inside the current package:**

- Platform cut: Tasks 0-5. This freezes the lifecycle contract, mapping,
  transport, WAL/fencing, shadow controller, and adapter-only managed-lane
  proof. No strategy behavior changes or live writer activation happen here.
- Canary cut: Tasks 6-8. This starts only after an explicit review checkpoint on
  the Task 0-5 evidence bundle.

## Decision Gates

| Gate | Resolved By | Blocks | Decision / Threshold |
| --- | --- | --- | --- |
| Gate 0: Ownership, claim, and lifecycle spec | Task 0 | Task 1 onward | intent state machine, ID chain, quarantine policy, and snapshot sequencing are locked |
| Gate 1: Writer-domain mapping spec | Task 1 | Task 2 onward | Manual enumeration rules for `controller_scope_id` and the first canary scope are locked |
| Gate 2: Same-host transport and latency budget | Task 2 | Task 5 onward | UDS transport chosen; added submit/cancel overhead p50 `<= 100us`, p99 `<= 750us`, backlog p99 `<= 2ms` |
| Gate 3: Synchronous ownership WAL and replay | Task 3 | Task 4 onward | Crash-safe append/replay/materialization, controller epoch fencing, and ownership-loss classification proven |
| Gate 4: Adapter-only managed-lane proof | Task 5 | Task 6 onward | external-client seam proves the ownership matrix without Nautilus core patches; explicit review checkpoint before strategy conversion |
| Gate 5: Attribution, failover, and rollback | Task 6 | Task 7 onward | lease-loss stop `<= 250ms`, `0` duplicate writes, `0` ambiguous owners after recovery, rollback works in one bounce |
| Gate 6: Active single-host canary | Task 8 | package complete | one full canary session with `0` unexplained ownership diffs |

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | in_review_quality | controller | none | `systems/flux/flux/common`, `systems/flux/flux/execution`, `systems/flux/flux/runners`, `systems/flux/flux/strategies`, `deploy/equities`, `deploy/*/systemd`, `deploy/tokenmm`, `ops/scripts`, `docs/runbooks`, `tests/unit_tests` | `codex/account-execution-controller-platform-cut-20260328` | `/home/ubuntu/nautilus_trader/.worktrees/account-execution-controller-platform-cut-20260328` | `f452fc5848..290b279c8a` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/common/test_account_projection.py` PASS; Task 0 verification commands all PASS on `8a9517decd`; Task 1 controller-scope checks PASS on `982daba8f1`; Task 2 transport/unit/benchmark checks PASS on `930723cce0`; Task 3 WAL/ledger/recovery checks PASS on `14dd18cc45`; Task 4 lease/runner/entrypoint checks PASS on `640707e8d9`; Task 5 adapter/unit/live checks PASS on `f63347ca91`; Task 6 verification reran green on `73762808a4`; Task 7 required strategy and runner verification commands PASS on `9540445e71`; `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_transport.py` PASS (`6 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/strategies/test_intent_publication.py` PASS (`8 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_node.py` PASS (`49 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_controller.py -k include_overnight_tags` FAIL-AS-EXPECTED (`TypeError: _IBKRActiveWriterGateway._place_order_async() got an unexpected keyword argument 'include_overnight'`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_controller.py -k include_overnight_tags` PASS (`1 passed, 12 deselected`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_controller.py` PASS (`14 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/live/test_execution_recon.py` PASS (`36 passed, 106 skipped`); `python ops/scripts/failover/controller_scope_failover.py --profile equities --scope ibkr.hedge.main --single-host --check-thresholds` PASS; `python ops/scripts/failover/controller_scope_failover.py --profile equities --scope ibkr.hedge.main --single-host --check-canary-session --session-artifact state/controller-canary/ibkr.hedge.main/session.json` PASS; `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/ops/test_controller_failover.py` PASS (`6 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_recovery.py` PASS (`10 passed`); `python ops/scripts/failover/controller_scope_failover.py --profile equities --scope ibkr.hedge.main --multi-box --check-thresholds` PASS; `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_leases.py` PASS (`4 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_wal.py` PASS (`3 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_ledger.py` PASS (`3 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py` FAIL (`3 failed, 56 passed`) on the same pre-existing unrelated monitoring/docs failures outside Task 11 scope; `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_node.py` PASS (`45 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_api.py` PASS (`33 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_controller.py` PASS (`7 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/api/test_balances_merge_dedupe.py` PASS (`12 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_tokenmm_risk_validation_contract.py` PASS (`4 passed`); `python ops/scripts/failover/controller_scope_failover.py --profile tokenmm --scope binance.pm.main --multi-box --check-thresholds` PASS; `git diff --check` PASS | 2026-03-29: local spec review on `f452fc5848..290b279c8a` found no remaining Task 11 scope or contract gaps after the blocker fix. The task is in quality review on the same pinned diff, with the same three unrelated stack-contract baseline failures explicitly excluded from Task 11 scope |
| Task 0: Lock ownership, claim, and lifecycle semantics before build | completed | controller | none | `systems/flux/flux/execution/intents.py`, `systems/flux/flux/execution/events.py`, `systems/flux/flux/execution/controller.py`, `tests/unit_tests/flux/execution/test_intents.py`, `tests/unit_tests/flux/execution/test_controller_state_machine.py`, `tests/unit_tests/flux/execution/test_snapshot_authority.py` | `codex/account-execution-controller-platform-cut-20260328` | `/home/ubuntu/nautilus_trader/.worktrees/account-execution-controller-platform-cut-20260328` | `ebe95d0836..8a9517decd` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_intents.py` PASS (`4 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_controller_state_machine.py` PASS (`3 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_snapshot_authority.py` PASS (`3 passed`) | 2026-03-28: spec review passed twice with no findings; quality review found and then cleared enum normalization, venue-origin round-trip, and public claim ID-chain enforcement issues; final head `8a9517decd` |
| Task 1: Lock writer-domain mapping and canary controller scope contracts | completed | controller | Task 0: Lock ownership, claim, and lifecycle semantics before build | `systems/flux/flux/common/controller_scopes.py`, `systems/flux/flux/common/account_scopes.py`, `systems/flux/flux/common/strategy_contracts.py`, `deploy/equities/equities.live.toml`, `tests/unit_tests/flux/common/test_controller_scopes.py`, `tests/unit_tests/examples/strategies/test_equities_stack_contract.py` | `codex/account-execution-controller-platform-cut-20260328` | `/home/ubuntu/nautilus_trader/.worktrees/account-execution-controller-platform-cut-20260328` | `6cb98172d2..982daba8f1` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/common/test_controller_scopes.py` PASS (`5 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_stack_contract.py` FAIL (`5 failed, 32 passed`) with the same unrelated installer/docs assertions still noisy on current `main`; `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_stack_contract.py -k 'controller_scope_contract or live_config_only_keeps_shared_contract_values'` PASS (`3 passed, 34 deselected`); `git diff --check -- systems/flux/flux/common/controller_scopes.py systems/flux/flux/common/account_scopes.py systems/flux/flux/common/strategy_contracts.py deploy/equities/equities.live.toml tests/unit_tests/flux/common/test_controller_scopes.py tests/unit_tests/examples/strategies/test_equities_stack_contract.py` PASS | 2026-03-28: spec review and quality review both passed with no findings; task completed on commit `982daba8f1`, with the pre-existing unrelated full-file stack-contract failures explicitly carried as background noise rather than Task 1 regressions |
| Task 2: Add UDS transport contracts and lock latency benchmark thresholds | completed | controller | Task 1: Lock writer-domain mapping and canary controller scope contracts | `systems/flux/flux/execution/transport.py`, `ops/scripts/bench/controller_intent_latency.py`, `tests/unit_tests/flux/execution/test_transport.py`, `tests/unit_tests/ops/test_controller_latency.py` | `codex/account-execution-controller-platform-cut-20260328` | `/home/ubuntu/nautilus_trader/.worktrees/account-execution-controller-platform-cut-20260328` | `491b83ffb8..930723cce0` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_transport.py` PASS (`5 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/ops/test_controller_latency.py` PASS (`2 passed`); `python ops/scripts/bench/controller_intent_latency.py --scenario baseline --check-budgets` PASS; `git diff --check -- systems/flux/flux/execution/transport.py ops/scripts/bench/controller_intent_latency.py tests/unit_tests/flux/execution/test_transport.py tests/unit_tests/ops/test_controller_latency.py` PASS | 2026-03-28: spec review passed twice with no findings; quality review found and then cleared accepted-reply identity and Linux UDS path-length issues; final head `930723cce0` |
| Task 3: Implement the synchronous ownership WAL, controller epoch fencing, and replay-safe materialization | completed | controller | Task 2: Add UDS transport contracts and lock latency benchmark thresholds | `systems/flux/flux/execution/wal.py`, `systems/flux/flux/execution/ledger.py`, `tests/unit_tests/flux/execution/test_wal.py`, `tests/unit_tests/flux/execution/test_ledger.py`, `tests/unit_tests/flux/execution/test_recovery.py` | `shared` | `/home/ubuntu/nautilus_trader/.worktrees/account-execution-controller-platform-cut-20260328` | `7cea76d9c0..14dd18cc45` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_wal.py` PASS (`3 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_ledger.py` PASS (`3 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_recovery.py` PASS (`7 passed`); `git diff --check 7cea76d9c0..14dd18cc45 -- systems/flux/flux/execution/wal.py systems/flux/flux/execution/ledger.py tests/unit_tests/flux/execution/test_wal.py tests/unit_tests/flux/execution/test_ledger.py tests/unit_tests/flux/execution/test_recovery.py` PASS | 2026-03-28: spec review passed after append-only WAL metadata fix `1446427343`; quality review found and then cleared the terminal `final_ack=True` ownership-release bug in `14dd18cc45`; final local quality pass found no remaining material issues |
| Task 4: Build the shadow controller runner with lease/fencing and single-host ingress gating | completed | controller | Task 3: Implement the synchronous ownership WAL, controller epoch fencing, and replay-safe materialization | `systems/flux/flux/execution/leases.py`, `systems/flux/flux/execution/controller.py`, `systems/flux/flux/runners/shared/controller_runner.py`, `systems/flux/flux/runners/equities/run_controller.py`, `tests/unit_tests/flux/execution/test_leases.py`, `tests/unit_tests/flux/runners/shared/test_controller_runner.py`, `tests/unit_tests/examples/strategies/test_equities_run_controller.py` | `shared` | `/home/ubuntu/nautilus_trader/.worktrees/account-execution-controller-platform-cut-20260328` | `2b239dc563..640707e8d9` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_leases.py` PASS (`4 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/runners/shared/test_controller_runner.py` PASS (`3 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_controller.py` PASS (`4 passed`); `git diff --check -- systems/flux/flux/execution/leases.py systems/flux/flux/execution/controller.py systems/flux/flux/runners/shared/controller_runner.py systems/flux/flux/runners/equities/run_controller.py tests/unit_tests/flux/execution/test_leases.py tests/unit_tests/flux/runners/shared/test_controller_runner.py tests/unit_tests/examples/strategies/test_equities_run_controller.py` PASS | 2026-03-28: spec review passed on `2b239dc563..255839759e`; quality review found and then cleared the duplicate-startup, post-TTL duplicate-start, and runner-lifetime ingress-gate holes in `11309da02c` and `640707e8d9`; final quality re-review on `2b239dc563..640707e8d9` reported no findings and Task 4 completed at head `640707e8d9` |
| Task 5: Integrate adapter-only controller-managed lanes and prove the ownership matrix | completed | controller | Task 4: Build the shadow controller runner with lease/fencing and single-host ingress gating | `systems/flux/flux/execution/nautilus_adapter.py`, `tests/unit_tests/flux/execution/test_nautilus_adapter.py`, `tests/unit_tests/live/test_execution_engine.py`, `tests/unit_tests/live/test_execution_recon.py` | `codex/account-execution-controller-platform-cut-20260328` | `/home/ubuntu/nautilus_trader/.worktrees/account-execution-controller-platform-cut-20260328` | `cbbfe408f8..f63347ca91` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_nautilus_adapter.py` PASS (`3 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/live/test_execution_engine.py` PASS (`22 passed, 56 skipped`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/live/test_execution_recon.py` PASS (`35 passed, 106 skipped`); `git diff --check -- systems/flux/flux/execution/nautilus_adapter.py tests/unit_tests/flux/execution/test_nautilus_adapter.py tests/unit_tests/live/test_execution_engine.py tests/unit_tests/live/test_execution_recon.py` PASS | 2026-03-28: implementation committed at `f63347ca91`; spec review passed with no findings on `cbbfe408f8..f63347ca91`; local quality review found no material issues in the pinned diff; residual risk limited to mixed managed/unmanaged startup mass-status coverage, which remains outside the current single-lane canary task scope |
| Task 6: Implement attribution, fencing invariants, and failover/rollback gates | completed | controller | Task 5: Integrate adapter-only controller-managed lanes and prove the ownership matrix | `systems/flux/flux/execution/attribution.py`, `systems/flux/flux/execution/controller.py`, `ops/scripts/bench/controller_intent_latency.py`, `ops/scripts/failover/controller_scope_failover.py`, `tests/unit_tests/flux/execution/test_attribution.py`, `tests/unit_tests/flux/execution/test_controller_fencing.py`, `tests/unit_tests/ops/test_controller_failover.py`, `tests/unit_tests/ops/test_controller_canary_rollout.py`, `tests/unit_tests/ops/test_controller_latency.py` | `codex/account-execution-controller-platform-cut-20260328` | `/home/ubuntu/nautilus_trader/.worktrees/account-execution-controller-platform-cut-20260328` | `f63347ca91..73762808a4` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_attribution.py` PASS (`5 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_controller_fencing.py` PASS (`4 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/ops/test_controller_failover.py` PASS (`2 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/ops/test_controller_canary_rollout.py` PASS (`4 passed`); `python ops/scripts/bench/controller_intent_latency.py --scenario canary --check-budgets` PASS; `python ops/scripts/failover/controller_scope_failover.py --profile equities --scope ibkr.hedge.main --single-host --check-thresholds` PASS; `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/ops/test_controller_latency.py -k canary` PASS (`1 passed, 2 deselected`); `python ops/scripts/failover/controller_scope_failover.py --profile equities --scope ibkr.hedge.main --single-host --check-canary-session --session-artifact <mismatched-temp-artifact>` FAIL-AS-EXPECTED (`returncode=1` on scope mismatch); `git diff --check -- systems/flux/flux/execution/attribution.py tests/unit_tests/flux/execution/test_attribution.py ops/scripts/failover/controller_scope_failover.py tests/unit_tests/ops/test_controller_canary_rollout.py docs/plans/2026-03-28-account-scoped-execution-controller.md` PASS | 2026-03-28: spec review found no Task 6 scope or contract mismatches on `f63347ca91..a52c49738d`; quality review found and the controller fixed the canary-session target-validation gap in `151a4fb67f` plus the zero-fill reservation-sign bug in `73762808a4`; final local quality review found no remaining material issues on `f63347ca91..73762808a4` |
| Task 7: Convert the canary strategy family into intent publishers plus canonical-state consumers | completed | controller | Task 6: Implement attribution, fencing invariants, and failover/rollback gates | `systems/flux/flux/strategies/makerv4/strategy.py`, `systems/flux/flux/runners/equities/run_node.py`, `tests/unit_tests/flux/strategies/test_intent_publication.py`, `tests/unit_tests/examples/strategies/test_equities_run_node.py` | `codex/account-execution-controller-platform-cut-20260328` | `/home/ubuntu/nautilus_trader/.worktrees/account-execution-controller-platform-cut-20260328` | `530d85c70f..9540445e71` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/strategies/test_intent_publication.py` FAIL-AS-EXPECTED on `4b8ed4d36d..working_tree` (`test_makerv4_on_start_hydrates_controller_state_without_background_feed_start` proved `on_start()` still called `feed.start()`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/strategies/test_intent_publication.py` PASS (`7 passed`) on `9540445e71`; `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_node.py` PASS (`47 passed`) on `9540445e71`; `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py` PASS (`25 passed`) on `9540445e71`; `git diff --check -- systems/flux/flux/strategies/makerv4/strategy.py systems/flux/flux/runners/equities/run_node.py tests/unit_tests/flux/strategies/test_intent_publication.py tests/unit_tests/examples/strategies/test_equities_run_node.py docs/plans/2026-03-28-account-scoped-execution-controller.md` PASS | 2026-03-28: TDD red was observed for missing `lifecycle_event_key()`, `canonical_state_key()`, and `sync_once()` on the runtime feed bridge; plan text was clarified so Task 7 explicitly ends at the consumer-side feed seam and independent spec review then passed on `4b8ed4d36d`; quality review reopened the task for the off-thread feed worker, which was removed in `9540445e71`; independent quality re-review then reported no remaining findings |
| Task 8: Activate controller write ownership for the single-host equities IBKR hedge canary | completed | controller | Task 7: Convert the canary strategy family into intent publishers plus canonical-state consumers | `systems/flux/flux/execution/controller.py`, `systems/flux/flux/execution/transport.py`, `systems/flux/flux/execution/ledger.py`, `systems/flux/flux/execution/nautilus_adapter.py`, `systems/flux/flux/runners/equities/run_node.py`, `systems/flux/flux/runners/equities/run_controller.py`, `systems/flux/flux/strategies/makerv4/strategy.py`, `deploy/equities/README.md`, `deploy/equities/equities.live.toml`, `deploy/equities/systemd/flux-equities.target`, `ops/scripts/deploy/install_equities_systemd.sh`, `tests/unit_tests/flux/execution/test_transport.py`, `tests/unit_tests/flux/strategies/test_intent_publication.py`, `tests/unit_tests/examples/strategies/test_equities_run_node.py`, `tests/unit_tests/examples/strategies/test_equities_run_controller.py`, `tests/unit_tests/live/test_execution_recon.py` | `codex/account-execution-controller-platform-cut-20260328` | `/home/ubuntu/nautilus_trader/.worktrees/account-execution-controller-platform-cut-20260328` | `9540445e71..working_tree` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_transport.py` PASS (`6 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/strategies/test_intent_publication.py` PASS (`8 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_node.py` PASS (`49 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_controller.py -k include_overnight_tags` FAIL-AS-EXPECTED (`TypeError: _IBKRActiveWriterGateway._place_order_async() got an unexpected keyword argument 'include_overnight'`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_controller.py -k include_overnight_tags` PASS (`1 passed, 12 deselected`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_controller.py` PASS (`14 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/live/test_execution_recon.py` PASS (`36 passed, 106 skipped`); `python ops/scripts/failover/controller_scope_failover.py --profile equities --scope ibkr.hedge.main --single-host --check-thresholds` PASS; `python ops/scripts/failover/controller_scope_failover.py --profile equities --scope ibkr.hedge.main --single-host --check-canary-session --session-artifact state/controller-canary/ibkr.hedge.main/session.json` PASS; `git diff --check` PASS | 2026-03-28: TDD red was observed for the active writer path before implementation. The user approved a broader Task 8 addendum after the narrow request/reply-only amendment proved insufficient. The transport now carries full controller command payloads, the node uses same-host UDS request/reply, the controller service is resident, accepted lifecycle plus canonical freshness markers publish into Redis, strategies refresh controller state on quote ticks, and the working tree now includes a concrete WAL-backed active writer path for controller-owned canary writes with rollback-to-shadow preserved. The controller-side spec review reopened only for missing `include_overnight` propagation in the default IBKR writer path; that gap is now closed. Two pinned spec-review agents timed out, so the controller completed the Task 8 spec review locally on the verified working tree and found no remaining blocking spec gaps. The fresh quality-review lane did not return before handoff, so the controller completed a local quality review on the same pinned diff, added missing controller-owned cancel-path regression coverage, and found no remaining material correctness or regression issues. `cancel_after_ms` remains transport-visible but is non-blocking for Task 8 because the approved canary scope requires a venue-write-capable request shape, not native IBKR expiry semantics, and the legacy hedge path does not translate `cancel_after_ms` either |
| Task 9: Switch canary read-side authority to controller-owned account truth and retire coexistence repair paths | not_started | unassigned | Task 8: Activate controller write ownership for the single-host equities IBKR hedge canary | `systems/flux/flux/runners/shared/profile_accounts.py`, `systems/flux/flux/runners/shared/portfolio_runner.py`, `systems/flux/flux/common/account_projection.py`, `systems/flux/flux/common/portfolio_snapshot.py`, `systems/flux/flux/api/app.py`, `docs/runbooks/equities-binance-perp-market-making.md`, `tests/unit_tests/flux/api/test_app.py`, `tests/unit_tests/flux/runners/shared/test_profile_accounts.py`, `tests/unit_tests/flux/common/test_portfolio_snapshot.py` | `shared` | `shared` | none | not_run | 2026-03-29: user approved remaining follow-on tasks. Task 9 is now unblocked, but it is deferred behind the TokenMM critical path because it touches later read-side/API surfaces and is not a dependency for Task 10 or Task 11 |
| Task 10: Add replicated ownership-log and multi-box stale-writer rejection hardening | completed | controller | Task 8: Activate controller write ownership for the single-host equities IBKR hedge canary | `systems/flux/flux/execution/wal.py`, `systems/flux/flux/execution/leases.py`, `ops/scripts/failover/controller_scope_failover.py`, `docs/runbooks/deploy-lanes.md`, `deploy/equities/README.md`, `tests/unit_tests/ops/test_controller_failover.py`, `tests/unit_tests/flux/execution/test_recovery.py` | `codex/account-execution-controller-platform-cut-20260328` | `/home/ubuntu/nautilus_trader/.worktrees/account-execution-controller-platform-cut-20260328` | `a4847a9799` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/ops/test_controller_failover.py` PASS (`6 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_recovery.py` PASS (`10 passed`); `python ops/scripts/failover/controller_scope_failover.py --profile equities --scope ibkr.hedge.main --multi-box --check-thresholds` PASS; `git diff --check` PASS | 2026-03-29: user approved continuing the follow-on packages to reach TokenMM production readiness. Task 10 started first because it is the gating dependency for Task 11 and the direct path to multi-box TokenMM migration. The Task 10 red cycle completed first on missing `--multi-box` CLI support plus missing replica-aware lease/WAL surfaces. A bounded external spec-review lane inspected the pinned diff but did not return a final verdict in time, so the controller completed a local spec pass on `873391bef3..working_tree` and found no Task 10 scope or contract gaps. A bounded external quality-review lane also did not return a usable verdict in time, so the controller completed a local quality review, tightened `restore_primary_from_replica()` to use SQLite backup semantics instead of raw file copy, added primary read proxies on `ReplicatedOwnershipWal`, reran the owned verification commands, and found no remaining material correctness or regression issues in Task 10 scope |
| Task 11: Migrate TokenMM shared Binance writer domains after the multi-box gates pass | in_review_quality | controller | Task 10: Add replicated ownership-log and multi-box stale-writer rejection hardening | `deploy/tokenmm/tokenmm.live.toml`, `deploy/tokenmm/systemd/flux-tokenmm.target`, `ops/scripts/deploy/install_tokenmm_systemd.sh`, `systems/flux/flux/runners/tokenmm/run_node.py`, `systems/flux/flux/runners/tokenmm/run_controller.py`, `systems/flux/flux/runners/tokenmm/run_api.py`, `systems/flux/flux/api/app.py`, `docs/runbooks/tokenmm-risk-validation.md`, `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`, `tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`, `tests/unit_tests/examples/strategies/test_tokenmm_run_api.py`, `tests/unit_tests/examples/strategies/test_tokenmm_run_controller.py`, `tests/unit_tests/flux/api/test_balances_merge_dedupe.py` | `codex/account-execution-controller-platform-cut-20260328` | `/home/ubuntu/nautilus_trader/.worktrees/account-execution-controller-platform-cut-20260328` | `f452fc5848..290b279c8a` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py` FAIL (`3 failed, 56 passed`) on the same pre-existing unrelated monitoring/docs failures outside Task 11 scope; `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_node.py` PASS (`45 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_api.py` PASS (`33 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_controller.py` PASS (`7 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/api/test_balances_merge_dedupe.py` PASS (`12 passed`); `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_tokenmm_risk_validation_contract.py` PASS (`4 passed`); `python ops/scripts/failover/controller_scope_failover.py --profile tokenmm --scope binance.pm.main --multi-box --check-thresholds` PASS; `git diff --check` PASS | 2026-03-29: local spec review on `f452fc5848..290b279c8a` found no remaining Task 11 scope or contract gaps after `290b279c8a`. The task is in local quality review on the same pinned diff, with the same three unrelated stack-contract baseline failures explicitly excluded from Task 11 scope |

---

### Task 0: Lock ownership, claim, and lifecycle semantics before build

**Files:**
- Create: `systems/flux/flux/execution/intents.py`
- Create: `systems/flux/flux/execution/events.py`
- Create: `systems/flux/flux/execution/controller.py`
- Create: `tests/unit_tests/flux/execution/test_intents.py`
- Create: `tests/unit_tests/flux/execution/test_controller_state_machine.py`
- Create: `tests/unit_tests/flux/execution/test_snapshot_authority.py`

**Dependencies:** `none`

**Write Scope:** `systems/flux/flux/execution/intents.py`, `systems/flux/flux/execution/events.py`, `systems/flux/flux/execution/controller.py`, `tests/unit_tests/flux/execution/test_intents.py`, `tests/unit_tests/flux/execution/test_controller_state_machine.py`, `tests/unit_tests/flux/execution/test_snapshot_authority.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_intents.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_controller_state_machine.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_snapshot_authority.py`

**Step 1: Write the failing ownership-contract tests**

Add tests that pin:

- the strategy/controller lifecycle states: `published`, `accepted`,
  `owned_pre_write`, `rejected`, `sent_to_venue`, `working`,
  `partially_filled`, `filled`, `canceled`, `quarantined`
- the deterministic ID chain:
  `intent_id -> controller_epoch -> controller_seq -> client_order_id -> venue_order_id`
- quarantine-first handling for external/manual/orphan venue activity
- distinct crash-window semantics for `owned_pre_write` versus `sent_to_venue`
- controller snapshot authority fields and monotonic sequencing requirements for
  any future read-side switch

**Step 2: Run the tests to verify they fail**

Run the commands above.

Expected: FAIL because the managed-lane ownership contract is not yet frozen.

**Step 3: Write the minimal implementation**

Implement:

- intent/event schemas that encode the lifecycle and ID chain
- controller-side ownership enums/state helpers
- snapshot authority contracts for `controller_scope_id`, `controller_epoch`,
  `controller_seq`, freshness, and authority state

**Step 4: Run the tests to verify they pass**

Run the commands above again.

Expected: PASS, with the execution contract frozen before transport, mapping, or
canary work begins.

**Step 5: Commit**

```bash
git add systems/flux/flux/execution/intents.py systems/flux/flux/execution/events.py systems/flux/flux/execution/controller.py tests/unit_tests/flux/execution/test_intents.py tests/unit_tests/flux/execution/test_controller_state_machine.py tests/unit_tests/flux/execution/test_snapshot_authority.py
git commit -m "feat(execution): lock controller ownership and lifecycle contracts"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 1: Lock writer-domain mapping and canary controller scope contracts

**Files:**
- Create: `systems/flux/flux/common/controller_scopes.py`
- Modify: `systems/flux/flux/common/account_scopes.py`
- Modify: `systems/flux/flux/common/strategy_contracts.py`
- Modify: `deploy/equities/equities.live.toml`
- Create: `tests/unit_tests/flux/common/test_controller_scopes.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`

**Dependencies:** `Task 0: Lock ownership, claim, and lifecycle semantics before build`

**Write Scope:** `systems/flux/flux/common/controller_scopes.py`, `systems/flux/flux/common/account_scopes.py`, `systems/flux/flux/common/strategy_contracts.py`, `deploy/equities/equities.live.toml`, `tests/unit_tests/flux/common/test_controller_scopes.py`, `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/common/test_controller_scopes.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_stack_contract.py`

**Step 1: Write the failing contract tests**

Add tests that pin:

- manual `controller_scope_id` enumeration rules
- the first canary scope as `equities / ibkr.hedge.main`
- when multiple logical account scopes may and may not share one controller
- invalid configs that map a strategy to a missing or conflicting writer domain

**Step 2: Run the tests to verify they fail**

Run the commands above.

Expected: FAIL because `controller_scope_id` contracts and canary mapping rules
do not exist yet.

**Step 3: Write the minimal implementation**

Implement:

- `ControllerScopeConfig`
- manual writer-domain manifest parsing
- validation helpers
- initial canary rows in the equities config

**Step 4: Run the tests to verify they pass**

Run the commands above again.

Expected: PASS, with writer-domain mapping no longer ambiguous for v1.

**Step 5: Commit**

```bash
git add systems/flux/flux/common/controller_scopes.py systems/flux/flux/common/account_scopes.py systems/flux/flux/common/strategy_contracts.py deploy/equities/equities.live.toml tests/unit_tests/flux/common/test_controller_scopes.py tests/unit_tests/examples/strategies/test_equities_stack_contract.py
git commit -m "feat(flux): lock controller writer-domain contracts"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Add UDS transport contracts and lock latency benchmark thresholds

**Files:**
- Create: `systems/flux/flux/execution/transport.py`
- Create: `ops/scripts/bench/controller_intent_latency.py`
- Create: `tests/unit_tests/flux/execution/test_transport.py`
- Create: `tests/unit_tests/ops/test_controller_latency.py`

**Dependencies:** `Task 1: Lock writer-domain mapping and canary controller scope contracts`

**Write Scope:** `systems/flux/flux/execution/transport.py`, `ops/scripts/bench/controller_intent_latency.py`, `tests/unit_tests/flux/execution/test_transport.py`, `tests/unit_tests/ops/test_controller_latency.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_transport.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/ops/test_controller_latency.py`
- `python ops/scripts/bench/controller_intent_latency.py --scenario baseline --check-budgets`

**Step 1: Write the failing transport tests**

Add tests that pin:

- UDS request/reply and event-stream transport contracts
- benchmark harness output shape
- explicit latency threshold checks

**Step 2: Run the tests to verify they fail**

Run the commands above.

Expected: FAIL because the transport and benchmark harness do not exist.

**Step 3: Write the minimal implementation**

Implement:

- v1 UDS transport contract
- benchmark harness with threshold assertions

**Step 4: Run the tests to verify they pass**

Run the commands above again.

Expected: PASS, with the transport and budgets fixed before strategy or canary
cutover work begins.

**Step 5: Commit**

```bash
git add systems/flux/flux/execution/transport.py ops/scripts/bench/controller_intent_latency.py tests/unit_tests/flux/execution/test_transport.py tests/unit_tests/ops/test_controller_latency.py
git commit -m "feat(flux): lock controller transport and latency budgets"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Implement the synchronous ownership WAL, controller epoch fencing, and replay-safe materialization

**Files:**
- Create: `systems/flux/flux/execution/wal.py`
- Create: `systems/flux/flux/execution/ledger.py`
- Create: `tests/unit_tests/flux/execution/test_wal.py`
- Create: `tests/unit_tests/flux/execution/test_ledger.py`
- Create: `tests/unit_tests/flux/execution/test_recovery.py`

**Dependencies:** `Task 2: Add UDS transport contracts and lock latency benchmark thresholds`

**Write Scope:** `systems/flux/flux/execution/wal.py`, `systems/flux/flux/execution/ledger.py`, `tests/unit_tests/flux/execution/test_wal.py`, `tests/unit_tests/flux/execution/test_ledger.py`, `tests/unit_tests/flux/execution/test_recovery.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_wal.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_ledger.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_recovery.py`

**Step 1: Write the failing recovery tests**

Add tests that require:

- durable append before owned venue write
- controller-epoch fence validation before append and before venue write
- replay after crash before venue write
- replay after venue write but before projection/materialization
- conservative pending-recovery behavior when the venue has partial truth but no
  final ack
- orphan classification when venue truth exists with no ownership record

**Step 2: Run the tests to verify they fail**

Run the commands above.

Expected: FAIL because no synchronous ownership WAL or recovery contract exists.

**Step 3: Write the minimal implementation**

Implement:

- SQLite ownership WAL
- controller epoch / fencing checks
- replay helpers
- async materialization interfaces
- explicit orphan classification rules

**Step 4: Run the tests to verify they pass**

Run the commands above again.

Expected: PASS, with the recovery contract strong enough for a controller to
claim canonical ownership on a single host.

**Step 5: Commit**

```bash
git add systems/flux/flux/execution/wal.py systems/flux/flux/execution/ledger.py tests/unit_tests/flux/execution/test_wal.py tests/unit_tests/flux/execution/test_ledger.py tests/unit_tests/flux/execution/test_recovery.py
git commit -m "feat(flux): add controller ownership wal fencing and recovery contract"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Build the shadow controller runner with lease/fencing and single-host ingress gating

**Files:**
- Create: `systems/flux/flux/execution/leases.py`
- Modify: `systems/flux/flux/execution/controller.py`
- Create: `systems/flux/flux/runners/shared/controller_runner.py`
- Create: `systems/flux/flux/runners/equities/run_controller.py`
- Create: `tests/unit_tests/flux/execution/test_leases.py`
- Create: `tests/unit_tests/flux/runners/shared/test_controller_runner.py`
- Create: `tests/unit_tests/examples/strategies/test_equities_run_controller.py`

**Dependencies:** `Task 3: Implement the synchronous ownership WAL, controller epoch fencing, and replay-safe materialization`

**Write Scope:** `systems/flux/flux/execution/leases.py`, `systems/flux/flux/execution/controller.py`, `systems/flux/flux/runners/shared/controller_runner.py`, `systems/flux/flux/runners/equities/run_controller.py`, `tests/unit_tests/flux/execution/test_leases.py`, `tests/unit_tests/flux/runners/shared/test_controller_runner.py`, `tests/unit_tests/examples/strategies/test_equities_run_controller.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_leases.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/runners/shared/test_controller_runner.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_controller.py`

**Step 1: Write the failing lease and runner tests**

Add tests that require:

- exactly one active lease owner per `controller_scope_id`
- local stale-writer rejection
- shadow-mode runner startup and shutdown
- single-host canary ingress gating to prevent accidental multi-writer startup

**Step 2: Run the tests to verify they fail**

Run the commands above.

Expected: FAIL because the controller runner and lease implementation do not
exist.

**Step 3: Write the minimal implementation**

Implement:

- lease/fencing abstraction
- shadow-mode controller runner
- single-host canary startup gates
- equities controller entrypoint

**Step 4: Run the tests to verify they pass**

Run the commands above again.

Expected: PASS, with shadow-mode controller lifecycle and lease ownership proven.

**Step 5: Commit**

```bash
git add systems/flux/flux/execution/leases.py systems/flux/flux/execution/controller.py systems/flux/flux/runners/shared/controller_runner.py systems/flux/flux/runners/equities/run_controller.py tests/unit_tests/flux/execution/test_leases.py tests/unit_tests/flux/runners/shared/test_controller_runner.py tests/unit_tests/examples/strategies/test_equities_run_controller.py
git commit -m "feat(flux): add shadow controller runner and lease fencing"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 5: Integrate adapter-only controller-managed lanes and prove the ownership matrix

**Files:**
- Create: `systems/flux/flux/execution/nautilus_adapter.py`
- Create: `tests/unit_tests/flux/execution/test_nautilus_adapter.py`
- Modify: `tests/unit_tests/live/test_execution_engine.py`
- Modify: `tests/unit_tests/live/test_execution_recon.py`

**Dependencies:** `Task 4: Build the shadow controller runner with lease/fencing and single-host ingress gating`

**Write Scope:** `systems/flux/flux/execution/nautilus_adapter.py`, `tests/unit_tests/flux/execution/test_nautilus_adapter.py`, `tests/unit_tests/live/test_execution_engine.py`, `tests/unit_tests/live/test_execution_recon.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_nautilus_adapter.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/live/test_execution_engine.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/live/test_execution_recon.py`

**Step 1: Write the failing ownership-matrix tests**

Add tests that require, for controller-managed lanes:

- controller owns startup reconciliation
- controller owns open-order truth
- controller owns external/orphan claim policy
- strategy-scoped Nautilus event semantics still work
- the external-client seam can disable or constrain conflicting legacy behavior
  without patching Nautilus core

**Step 2: Run the tests to verify they fail**

Run the commands above.

Expected: FAIL because controller-managed ownership boundaries are not yet
encoded at the adapter seam.

**Step 3: Write the minimal implementation**

Implement a controller/Nautilus compatibility layer that:

- uses the external-client seam
- keeps strategy event semantics
- disables or bypasses conflicting strategy-local ownership on managed lanes
- does not patch `nautilus_trader/execution/engine.pyx` or
  `nautilus_trader/live/execution_engine.py`

If an invariant cannot be enforced here, stop and open an explicit design
addendum for the smallest possible Nautilus core patch rather than extending
this task ad hoc.

**Step 4: Run the tests to verify they pass**

Run the commands above again.

Expected: PASS, with one clear owner per responsibility on managed lanes.

**Step 5: Commit**

```bash
git add systems/flux/flux/execution/nautilus_adapter.py tests/unit_tests/flux/execution/test_nautilus_adapter.py tests/unit_tests/live/test_execution_engine.py tests/unit_tests/live/test_execution_recon.py
git commit -m "feat(execution): add adapter-managed controller ownership path"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 6: Implement attribution, fencing invariants, and failover/rollback gates

**Package Note:** This task begins the canary cut and requires an explicit
review checkpoint on the Task 0-5 platform cut before implementation starts.

**Files:**
- Create: `systems/flux/flux/execution/attribution.py`
- Modify: `systems/flux/flux/execution/controller.py`
- Modify: `ops/scripts/bench/controller_intent_latency.py`
- Create: `ops/scripts/failover/controller_scope_failover.py`
- Create: `tests/unit_tests/flux/execution/test_attribution.py`
- Create: `tests/unit_tests/flux/execution/test_controller_fencing.py`
- Create: `tests/unit_tests/ops/test_controller_failover.py`
- Create: `tests/unit_tests/ops/test_controller_canary_rollout.py`
- Modify: `tests/unit_tests/ops/test_controller_latency.py`

**Dependencies:** `Task 5: Integrate adapter-only controller-managed lanes and prove the ownership matrix`

**Write Scope:** `systems/flux/flux/execution/attribution.py`, `systems/flux/flux/execution/controller.py`, `ops/scripts/bench/controller_intent_latency.py`, `ops/scripts/failover/controller_scope_failover.py`, `tests/unit_tests/flux/execution/test_attribution.py`, `tests/unit_tests/flux/execution/test_controller_fencing.py`, `tests/unit_tests/ops/test_controller_failover.py`, `tests/unit_tests/ops/test_controller_canary_rollout.py`, `tests/unit_tests/ops/test_controller_latency.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_attribution.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_controller_fencing.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/ops/test_controller_failover.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/ops/test_controller_canary_rollout.py`
- `python ops/scripts/bench/controller_intent_latency.py --scenario canary --check-budgets`
- `python ops/scripts/failover/controller_scope_failover.py --profile equities --scope ibkr.hedge.main --single-host --check-thresholds`

**Step 1: Write the failing gate tests**

Add tests that require:

- explicit shared-netting fill allocation and reservation rules
- fencing-epoch validation before every outbound write
- lease-loss stop thresholds
- single-host split-brain rejection
- rollback switch behavior
- shadow parity acceptance accounting

**Step 2: Run the tests to verify they fail**

Run the commands above.

Expected: FAIL because attribution, fencing, failover, and rollback surfaces are
not yet complete.

**Step 3: Write the minimal implementation**

Implement:

- attribution engine
- controller fencing enforcement
- failover drill harness
- canary rollout harness

**Step 4: Run the tests to verify they pass**

Run the commands above again.

Expected: PASS, with all active-cutover gates proven before strategy intent
conversion or a live writer canary.

**Step 5: Commit**

```bash
git add systems/flux/flux/execution/attribution.py systems/flux/flux/execution/controller.py ops/scripts/failover/controller_scope_failover.py tests/unit_tests/flux/execution/test_attribution.py tests/unit_tests/flux/execution/test_controller_fencing.py tests/unit_tests/ops/test_controller_failover.py tests/unit_tests/ops/test_controller_canary_rollout.py
git commit -m "feat(flux): prove controller attribution fencing and canary gates"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 7: Convert the canary strategy family into intent publishers plus canonical-state consumers

**Files:**
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `systems/flux/flux/runners/equities/run_node.py`
- Create: `tests/unit_tests/flux/strategies/test_intent_publication.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_node.py`

**Dependencies:** `Task 6: Implement attribution, fencing invariants, and failover/rollback gates`

**Write Scope:** `systems/flux/flux/strategies/makerv4/strategy.py`, `systems/flux/flux/runners/equities/run_node.py`, `tests/unit_tests/flux/strategies/test_intent_publication.py`, `tests/unit_tests/examples/strategies/test_equities_run_node.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/strategies/test_intent_publication.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_node.py`

**Step 1: Write the failing strategy tests**

Add tests that require, for the first canary strategy family:

- place/cancel/hedge actions to publish controller intents
- strategies to retain local one-cycle-late shadow state only
- strategies to consume controller lifecycle callbacks and canonical exposure
  state through the Task 7 consumer-side feed contract
- runners to attach controller endpoints and canonical-state feeds using the
  agreed consumer-side contract, while leaving live controller-side feed
  publication to Task 8 active-writer activation surfaces

**Step 2: Run the tests to verify they fail**

Run the commands above.

Expected: FAIL because strategies still submit directly and assume canonical
order ownership.

**Step 3: Write the minimal implementation**

Convert strategies into:

- intent publishers
- canonical-state consumers
- temporary compatibility balance publishers

Task 7 explicitly stops at the strategy / runner consumer seam. It may define
and test the consumer-side runtime feed contract, but it must not widen into
controller runtime publication or active-writer activation surfaces, which stay
in Task 8.

**Step 4: Run the tests to verify they pass**

Run the commands above again.

Expected: PASS, with strategies now aligned to the controller ownership model
that was frozen in Tasks 0-6, without pulling TokenMM runtime changes into the
first package.

**Step 5: Commit**

```bash
git add systems/flux/flux/strategies/makerv4/strategy.py systems/flux/flux/runners/equities/run_node.py tests/unit_tests/flux/strategies/test_intent_publication.py tests/unit_tests/examples/strategies/test_equities_run_node.py
git commit -m "feat(strategies): publish canary controller intents and consume canonical state"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 8: Activate controller write ownership for the single-host equities IBKR hedge canary

**Files:**
- Modify: `systems/flux/flux/execution/controller.py`
- Modify: `systems/flux/flux/execution/transport.py`
- Modify: `systems/flux/flux/execution/ledger.py`
- Modify: `systems/flux/flux/execution/nautilus_adapter.py`
- Modify: `systems/flux/flux/runners/equities/run_node.py`
- Modify: `systems/flux/flux/runners/equities/run_controller.py`
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `deploy/equities/README.md`
- Modify: `deploy/equities/equities.live.toml`
- Modify: `deploy/equities/systemd/flux-equities.target`
- Modify: `ops/scripts/deploy/install_equities_systemd.sh`
- Modify: `tests/unit_tests/flux/execution/test_transport.py`
- Modify: `tests/unit_tests/flux/strategies/test_intent_publication.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_node.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_controller.py`
- Modify: `tests/unit_tests/live/test_execution_recon.py`

**Dependencies:** `Task 7: Convert the canary strategy family into intent publishers plus canonical-state consumers`

**Write Scope:** `systems/flux/flux/execution/controller.py`, `systems/flux/flux/execution/transport.py`, `systems/flux/flux/execution/ledger.py`, `systems/flux/flux/execution/nautilus_adapter.py`, `systems/flux/flux/runners/equities/run_node.py`, `systems/flux/flux/runners/equities/run_controller.py`, `systems/flux/flux/strategies/makerv4/strategy.py`, `deploy/equities/README.md`, `deploy/equities/equities.live.toml`, `deploy/equities/systemd/flux-equities.target`, `ops/scripts/deploy/install_equities_systemd.sh`, `tests/unit_tests/flux/execution/test_transport.py`, `tests/unit_tests/flux/strategies/test_intent_publication.py`, `tests/unit_tests/examples/strategies/test_equities_run_node.py`, `tests/unit_tests/examples/strategies/test_equities_run_controller.py`, `tests/unit_tests/live/test_execution_recon.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_transport.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/strategies/test_intent_publication.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_node.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_controller.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/live/test_execution_recon.py`
- `python ops/scripts/failover/controller_scope_failover.py --profile equities --scope ibkr.hedge.main --single-host --check-thresholds`
- `python ops/scripts/failover/controller_scope_failover.py --profile equities --scope ibkr.hedge.main --single-host --check-canary-session --session-artifact state/controller-canary/ibkr.hedge.main/session.json`

**Step 1: Write the failing active-writer tests**

Add tests that require:

- the IBKR hedge canary to flip from shadow to active writer mode
- same-host request/reply transport to carry controller intents from the node
  process into the controller process instead of the process-local msgbus
- the request payload to carry enough controller command data to build the
  actual venue submit/cancel command, not only `ExecutionIntent`
- the controller service to stay resident and serve the canary scope while it
  owns the lease
- a concrete canary-scoped controller-owned venue write path behind the
  existing `ExecutionVenueWriter` seam
- compatibility outputs to remain enabled
- rollback toggle to disable controller write ownership without deleting the
  shadow path
- ongoing controller lifecycle/canonical publication into the strategy bridge
  instead of startup-only `sync_once()` hydration
- canary-session audit output with `0` unexplained ownership diffs

**Step 2: Run the tests to verify they fail**

Run the commands above.

Expected: FAIL because controller write ownership is not yet activatable.

**Step 3: Write the minimal implementation**

Implement:

- active-writer mode toggle for the canary
- same-host request/reply publication from `run_node.py` into the controller
  runtime using the Task 2 transport envelopes
- request/reply payload widening or an equivalent venue-write-capable request
  shape so the controller receives the full command data it must execute
- resident controller listener/service startup in `run_controller.py` for the
  canary scope, without widening into read-side authority or TokenMM runtime
- concrete controller-owned venue writes for the canary behind the existing
  `ExecutionVenueWriter` seam and WAL/ledger ownership flow
- ongoing controller lifecycle/canonical publication into the strategy bridge
  so controller-managed strategies are not limited to startup-only hydration
- deploy-manifest and systemd activation surfaces
- single-host rollback switch

**Step 4: Run the tests to verify they pass**

Run the commands above again.

Expected: PASS, with the canary able to become the sole writer while retaining
compatibility outputs and rollback, and with one recorded canary-session
artifact showing `0` unexplained ownership diffs.

**Step 5: Commit**

```bash
git add systems/flux/flux/execution/controller.py systems/flux/flux/execution/transport.py systems/flux/flux/execution/ledger.py systems/flux/flux/execution/nautilus_adapter.py systems/flux/flux/runners/equities/run_node.py systems/flux/flux/runners/equities/run_controller.py systems/flux/flux/strategies/makerv4/strategy.py deploy/equities/README.md deploy/equities/equities.live.toml deploy/equities/systemd/flux-equities.target ops/scripts/deploy/install_equities_systemd.sh tests/unit_tests/flux/execution/test_transport.py tests/unit_tests/flux/strategies/test_intent_publication.py tests/unit_tests/examples/strategies/test_equities_run_node.py tests/unit_tests/examples/strategies/test_equities_run_controller.py tests/unit_tests/live/test_execution_recon.py
git commit -m "feat(equities): activate ibkr hedge controller canary"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 9: Switch canary read-side authority to controller-owned account truth and retire coexistence repair paths

**Package Note:** This task is outside the current execution package and
requires explicit follow-on approval after Task 8.

**Files:**
- Modify: `systems/flux/flux/runners/shared/profile_accounts.py`
- Modify: `systems/flux/flux/runners/shared/portfolio_runner.py`
- Modify: `systems/flux/flux/common/account_projection.py`
- Modify: `systems/flux/flux/common/portfolio_snapshot.py`
- Modify: `systems/flux/flux/api/app.py`
- Modify: `docs/runbooks/equities-binance-perp-market-making.md`
- Modify: `tests/unit_tests/flux/api/test_app.py`
- Modify: `tests/unit_tests/flux/runners/shared/test_profile_accounts.py`
- Modify: `tests/unit_tests/flux/common/test_portfolio_snapshot.py`

**Dependencies:** `Task 8: Activate controller write ownership for the single-host equities IBKR hedge canary`

**Write Scope:** `systems/flux/flux/runners/shared/profile_accounts.py`, `systems/flux/flux/runners/shared/portfolio_runner.py`, `systems/flux/flux/common/account_projection.py`, `systems/flux/flux/common/portfolio_snapshot.py`, `systems/flux/flux/api/app.py`, `docs/runbooks/equities-binance-perp-market-making.md`, `tests/unit_tests/flux/api/test_app.py`, `tests/unit_tests/flux/runners/shared/test_profile_accounts.py`, `tests/unit_tests/flux/common/test_portfolio_snapshot.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/runners/shared/test_profile_accounts.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/common/test_portfolio_snapshot.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/api/test_app.py`

**Step 1: Write the failing read-side authority tests**

Add tests that require:

- controller-owned account truth to become authoritative for the canary
- controller snapshots to expose monotonic `controller_seq` and freshness
  markers that let the read side reject stale legacy rows deterministically
- explicit degraded behavior when controller and legacy compatibility outputs
  diverge
- retirement of canary-only balance repair logic that is no longer needed

**Step 2: Run the tests to verify they fail**

Run the commands above.

Expected: FAIL because the read side still treats controller outputs as a
compatibility source rather than the authoritative canary truth.

**Step 3: Write the minimal implementation**

Implement:

- controller-first read-side precedence for the canary
- deterministic sequence/freshness-based stale-row rejection
- explicit degraded fallback behavior
- retirement of coexistence repair paths for the canary

**Step 4: Run the tests to verify they pass**

Run the commands above again.

Expected: PASS, with the canary proving end-to-end controller-owned shared
account truth.

**Step 5: Commit**

```bash
git add systems/flux/flux/runners/shared/profile_accounts.py systems/flux/flux/runners/shared/portfolio_runner.py systems/flux/flux/common/account_projection.py systems/flux/flux/common/portfolio_snapshot.py systems/flux/flux/api/app.py docs/runbooks/equities-binance-perp-market-making.md tests/unit_tests/flux/api/test_app.py tests/unit_tests/flux/runners/shared/test_profile_accounts.py tests/unit_tests/flux/common/test_portfolio_snapshot.py
git commit -m "feat(flux): switch canary read side to controller authority"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 10: Add replicated ownership-log and multi-box stale-writer rejection hardening

**Package Note:** This task is outside the current execution package and
requires explicit follow-on approval after Task 8.

**Files:**
- Modify: `systems/flux/flux/execution/wal.py`
- Modify: `systems/flux/flux/execution/leases.py`
- Modify: `ops/scripts/failover/controller_scope_failover.py`
- Modify: `docs/runbooks/deploy-lanes.md`
- Modify: `deploy/equities/README.md`
- Modify: `tests/unit_tests/ops/test_controller_failover.py`
- Modify: `tests/unit_tests/flux/execution/test_recovery.py`

**Dependencies:** `Task 8: Activate controller write ownership for the single-host equities IBKR hedge canary`

**Write Scope:** `systems/flux/flux/execution/wal.py`, `systems/flux/flux/execution/leases.py`, `ops/scripts/failover/controller_scope_failover.py`, `docs/runbooks/deploy-lanes.md`, `deploy/equities/README.md`, `tests/unit_tests/ops/test_controller_failover.py`, `tests/unit_tests/flux/execution/test_recovery.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/ops/test_controller_failover.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/execution/test_recovery.py`
- `python ops/scripts/failover/controller_scope_failover.py --profile equities --scope ibkr.hedge.main --multi-box --check-thresholds`

**Step 1: Write the failing multi-box tests**

Add tests that require:

- replicated ownership logging or equivalent multi-host durability
- stale-writer rejection under lease loss and partition drills
- zero duplicate writes in multi-box failover simulation

**Step 2: Run the tests to verify they fail**

Run the commands above.

Expected: FAIL because the canary only has single-host ownership logging and
split-brain protection today.

**Step 3: Write the minimal implementation**

Implement:

- replicated ownership logging or equivalent durability for multi-box mode
- stronger stale-writer rejection
- multi-box drill updates and deploy-lane docs

**Step 4: Run the tests to verify they pass**

Run the commands above again.

Expected: PASS, with multi-box hardening proven before TokenMM migration.

**Step 5: Commit**

```bash
git add systems/flux/flux/execution/wal.py systems/flux/flux/execution/leases.py ops/scripts/failover/controller_scope_failover.py docs/runbooks/deploy-lanes.md deploy/equities/README.md tests/unit_tests/ops/test_controller_failover.py tests/unit_tests/flux/execution/test_recovery.py
git commit -m "feat(flux): harden controller ownership for multi-box failover"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 11: Migrate TokenMM shared Binance writer domains after the multi-box gates pass

**Package Note:** This task is outside the current execution package and
requires explicit follow-on approval after Task 10.

**Addendum (approved 2026-03-29):** The original Task 11 write scope was too
narrow to complete the real TokenMM ownership handoff. This task now includes
the TokenMM runner seam in `systems/flux/flux/runners/tokenmm/run_node.py` and
dedicated controller-runner tests so the managed Binance nodes can hand intent
publication and canonical-state consumption to the controller path, disable
node-owned execution/startup reconciliation for controller-managed lanes, and
let `systems/flux/flux/runners/tokenmm/run_controller.py` become the actual
request/reply, venue-write, and startup-reconciliation owner for
`tokenmm.binance.pm.main`. Keep this addendum narrow: prefer runner-layer
shims over `makerv3` strategy edits, and only widen into strategy internals if
the failing red cycle proves the runner seam is insufficient.

**Files:**
- Modify: `deploy/tokenmm/tokenmm.live.toml`
- Modify: `deploy/tokenmm/systemd/flux-tokenmm.target`
- Modify: `ops/scripts/deploy/install_tokenmm_systemd.sh`
- Modify: `systems/flux/flux/runners/tokenmm/run_node.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_controller.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_api.py`
- Modify: `systems/flux/flux/api/app.py`
- Modify: `docs/runbooks/tokenmm-risk-validation.md`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_run_api.py`
- Create: `tests/unit_tests/examples/strategies/test_tokenmm_run_controller.py`
- Modify: `tests/unit_tests/flux/api/test_balances_merge_dedupe.py`

**Dependencies:** `Task 10: Add replicated ownership-log and multi-box stale-writer rejection hardening`

**Write Scope:** `deploy/tokenmm/tokenmm.live.toml`, `deploy/tokenmm/systemd/flux-tokenmm.target`, `ops/scripts/deploy/install_tokenmm_systemd.sh`, `systems/flux/flux/runners/tokenmm/run_node.py`, `systems/flux/flux/runners/tokenmm/run_controller.py`, `systems/flux/flux/runners/tokenmm/run_api.py`, `systems/flux/flux/api/app.py`, `docs/runbooks/tokenmm-risk-validation.md`, `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`, `tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`, `tests/unit_tests/examples/strategies/test_tokenmm_run_api.py`, `tests/unit_tests/examples/strategies/test_tokenmm_run_controller.py`, `tests/unit_tests/flux/api/test_balances_merge_dedupe.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_api.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_controller.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/api/test_balances_merge_dedupe.py`
- `python ops/scripts/failover/controller_scope_failover.py --profile tokenmm --scope binance.pm.main --multi-box --check-thresholds`

**Step 1: Write the failing TokenMM migration tests**

Add tests that require:

- TokenMM shared Binance writer domains to bind to active controllers
- API balances to emit one authoritative shared Binance collateral row from
  controller-owned account truth
- managed Binance nodes to disable node-owned execution and startup
  reconciliation in favor of a controller request/reply seam
- the TokenMM controller runner to own startup reconciliation and venue writes
  for `tokenmm.binance.pm.main`

**Step 2: Run the tests to verify they fail**

Run the commands above.

Expected: FAIL because TokenMM still uses strategy-local execution ownership for
shared Binance writer domains.

**Step 3: Write the minimal implementation**

Implement:

- TokenMM controller binding and systemd surfaces
- runner-layer TokenMM controller intent publication and canonical-state
  consumption for managed Binance lanes
- controller-owned startup reconciliation and venue-write handling for the
  managed Binance scope
- controller-owned API/readiness truth for shared Binance domains
- rollout docs for live validation and promotion

**Step 4: Run the tests to verify they pass**

Run the commands above again.

Expected: PASS, with TokenMM shared Binance account truth now owned by the
controller path.

**Step 5: Commit**

```bash
git add deploy/tokenmm/tokenmm.live.toml deploy/tokenmm/systemd/flux-tokenmm.target ops/scripts/deploy/install_tokenmm_systemd.sh systems/flux/flux/runners/tokenmm/run_node.py systems/flux/flux/runners/tokenmm/run_controller.py systems/flux/flux/runners/tokenmm/run_api.py systems/flux/flux/api/app.py docs/runbooks/tokenmm-risk-validation.md tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py tests/unit_tests/examples/strategies/test_tokenmm_run_node.py tests/unit_tests/examples/strategies/test_tokenmm_run_api.py tests/unit_tests/examples/strategies/test_tokenmm_run_controller.py tests/unit_tests/flux/api/test_balances_merge_dedupe.py
git commit -m "feat(tokenmm): migrate shared Binance writer domains to controllers"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.
