# Equities Market-Data Recovery V1 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Restore honest real-time equities signal flow and full market-data recovery on grouped nodes by moving quote-reset ownership from individual strategies to a shared node-scoped recovery layer.

**Architecture:** Keep the March 27 grouped-node topology and all external `equities` contracts. Add one shared quote-feed supervisor per node keyed by feed identity, rewire `MakerV4Strategy` to report staleness instead of directly resetting feeds, harden Hyperliquid and Binance adapters for idempotent replay-first recovery, and make `recovering/down` first-class fail-closed states in readiness and signal payloads with cancel-only safety when required feeds are non-tradeable.

**Tech Stack:** Python, Nautilus live runners, Flux equities strategies, venue adapters for Hyperliquid/Binance/IBKR, pytest, immutable release-root deploys, systemd/Pulse.

## External Review Context

This plan is intentionally self-contained for reviewers who were not part of the incident investigation.

Why this plan exists:

1. After the grouped-node rollout, the public equities Signal surface and the direct equities signals API could both show rows frozen for minutes or longer even though the page still appeared live.
2. Producer-side inspection showed the stale data existed before API serialization, so the root problem was not frontend caching.
3. Restarting one grouped node could briefly revive timestamps and then the node would flatten again, which pointed to silent quote-subscription/runtime stalls.
4. The branch already contains honesty fixes so stale rows no longer claim to be fresh/good incorrectly. Those fixes are necessary, but they are not sufficient for prod trading V1 because they do not restore quote movement.
5. The remaining unresolved issue is shared market-data recovery under grouped maker/taker nodes.

What is already considered decided for this review:

- do not change `/equities` or the external equities API contract in this wave
- do not undo the grouped-node topology as the primary fix
- do not treat the earlier dual-IBKR-gateway cleanup as the architectural answer to the remaining problem
- do not rely on more strategy-local resubscribe patches as the long-term recovery model

Review provenance note:

- one external review referenced the upstream public NautilusTrader project; this plan only adopts points that were verified against this fork, such as existing `ComponentState.degrade()` / `fault()` support and adapter reconnect-replay behavior

What an external reviewer should challenge:

- ownership boundaries between strategies, runner, supervisor, and venue adapters
- fail-closed safety for live trading behavior, not just UI honesty
- testability of the state machine and grouped-node behavior
- rollout, rollback, and observability quality for production cutover

**Context Docs:**
- Design: `docs/plans/2026-03-28-equities-market-data-recovery-v1-design.md`
- PRD: `none`
- Relevant specs/runbooks: `docs/plans/2026-03-27-equities-shared-symbol-node-design.md`, `docs/plans/2026-03-27-equities-shared-symbol-node.md`, `deploy/equities/README.md`, `deploy/equities/strategies/README.md`, `systems/flux/docs/api.md`, `tests/unit_tests/examples/strategies/test_equities_run_node.py`, `tests/unit_tests/examples/strategies/test_equities_readiness.py`, `tests/unit_tests/flux/strategies/equities_maker/test_strategy.py`

**Decision Summary:**
- Do not continue with strategy-owned resubscribe patches as the primary recovery model.
- Do not build a standalone venue daemon for V1.
- Add an explicit runner-to-strategy attachment seam for node-scoped quote recovery objects before wiring supervisor behavior.
- Add a shared node-scoped quote-feed supervisor keyed by feed identity and make strategies observer-only for recovery.
- Centralize subscribe, reset, and final unsubscribe side effects at the shared supervisor layer for V1.
- Keep IBKR at the shared publisher/session boundary in V1; per-node recovery hardening targets Hyperliquid and Binance maker feeds.
- Require pair-level tradeability gating plus cancel-only quote pull when required feeds are non-tradeable.
- Require adapter outcomes to re-enter the runner/kernel thread before mutating supervisor state.
- Reuse existing component degrade/fault lifecycle rails instead of inventing a new operator lifecycle model.
- Hyperliquid cache rehydrate and idempotent reset semantics are mandatory V1 scope at the actual client layer.
- Binance idempotent reset semantics are mandatory V1 scope.
- `recovering` must be added without regressing the existing fail-closed `down` contract.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | not_started | unassigned | Plan created |
| Task 1: Add A Strategy Attachment Seam And Node-Scoped Quote Feed Supervisor Contract | not_started | unassigned | Plan created |
| Task 2: Rewire MakerV4 To Use Shared Recovery Ownership For Full Quote Lifecycle | not_started | unassigned | Plan created |
| Task 3: Harden Hyperliquid Recovery Semantics | not_started | unassigned | Plan created |
| Task 4: Harden Binance Recovery Semantics | not_started | unassigned | Plan created |
| Task 5: Add Honest Recovering-State Semantics Without Regressing Down-State Behavior | not_started | unassigned | Plan created |
| Task 6: Validate Live Recovery And Cut The V1 Prod Release | not_started | unassigned | Plan created |

---

### Task 1: Add A Strategy Attachment Seam And Node-Scoped Quote Feed Supervisor Contract

**Files:**
- Create: `systems/flux/flux/runners/shared/quote_feed_supervisor.py`
- Create: `tests/unit_tests/examples/strategies/test_quote_feed_supervisor.py`
- Modify: `systems/flux/flux/runners/equities/run_node.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_node.py`
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `tests/unit_tests/flux/strategies/equities_maker/test_strategy.py`

**Step 1: Write the failing attachment-seam and supervisor tests**

Add focused tests for a new node-scoped recovery object that prove:

- two grouped siblings requesting recovery for the same feed identity produce one reset action
- same-instrument feeds with different feed identities do not alias into one supervisor record
- the supervisor is the sole owner of lifecycle state (`desired`, `state`, `attempt_count`, `backoff_until`, `last_error_summary`)
- the strictest active freshness budget drives local unusable state, but node-level reset admission stays feed-scoped
- a fresh quote transitions `bootstrapping/recovering` back to `healthy`
- repeated failed recovery attempts transition the feed to `down`
- missing startup/session preconditions transition the feed to `blocked` without reset storms
- a node-local venue/session blocker suppresses per-feed reset storms when a shared venue precondition is unhealthy
- unregistering a strategy claimant removes its influence on budget and ownership
- loss of one sibling strategy does not prevent the shared feed from continuing to advance health state

Add strategy-level tests that prove `MakerV4Strategy` exposes an explicit attachment seam for a quote-feed supervisor/control object without yet changing liveness behavior.

Also extend `test_equities_run_node.py` so one grouped node proves both attached strategies receive the same supervisor instance and shared control emitter.
Also prove the refactor preserves each strategy's local quote-topic attachment path so quote ticks still reach `on_quote_tick` after external feed ownership moves to the shared supervisor.

**Step 2: Run the tests to confirm they fail**

Run: `./.venv.py312/bin/python -m pytest -q tests/unit_tests/examples/strategies/test_quote_feed_supervisor.py tests/unit_tests/examples/strategies/test_equities_run_node.py -k 'quote_feed_supervisor or shared_recovery'`

Expected: FAIL because there is no supervisor module, no explicit strategy attachment seam, and the runner does not inject one.

**Step 3: Implement the minimal attachment seam and supervisor**

Create `quote_feed_supervisor.py` with:

- one feed-state record per feed identity
- claimant registration and deregistration
- last quote timestamp tracking
- `bootstrapping/healthy/stale/blocked/recovering/down` state transitions
- bounded recovery backoff and attempt counting
- one injected reset callable per feed identity
- feed-scoped reset admission policy kept separate from claimant-local unusable thresholds
- node-local venue/session blocker support
- a runner-owned ingress path for adapter/reset outcomes so supervisor state stays on the node/kernel thread

Add explicit strategy attachment methods in `MakerV4Strategy` so the runner can wire the shared supervisor and control emitter without reaching into internals.

Keep the first implementation synchronous and in-process. Do not introduce Redis, Pulse, or a new service layer here.

**Step 4: Wire the supervisor into the equities runner**

Update `run_node.py` so:

- one supervisor is created per built node
- each attached strategy receives the shared supervisor during construction/attachment
- grouped siblings register the same maker and reference feed identities with the supervisor
- reset ownership is canonical and node-scoped
- the injected reset callable is runner-owned rather than borrowed from an arbitrary sibling strategy object
- adapter/reset outcomes re-enter supervisor state through the runner-owned ingress path rather than mutating it directly from background tasks

**Step 5: Re-run the tests**

Run: `./.venv.py312/bin/python -m pytest -q tests/unit_tests/examples/strategies/test_quote_feed_supervisor.py tests/unit_tests/examples/strategies/test_equities_run_node.py -k 'quote_feed_supervisor or shared_recovery'`

Expected: PASS

**Step 6: Commit**

```bash
git add systems/flux/flux/runners/shared/quote_feed_supervisor.py \
  systems/flux/flux/runners/equities/run_node.py \
  systems/flux/flux/strategies/makerv4/strategy.py \
  tests/unit_tests/examples/strategies/test_quote_feed_supervisor.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/flux/strategies/equities_maker/test_strategy.py
git commit -m "feat: add node-scoped quote feed supervisor"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Rewire MakerV4 To Use Shared Recovery Ownership For Full Quote Lifecycle

**Files:**
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `tests/unit_tests/flux/strategies/equities_maker/test_strategy.py`
- Modify: `tests/unit_tests/flux/strategies/equities_taker/test_strategy.py`

**Step 1: Write the failing strategy tests**

Add tests that prove:

- `on_start` no longer directly owns external venue subscribe side effects
- `on_stop` no longer directly owns external venue unsubscribe side effects
- `on_time_event` no longer directly owns external venue reset side effects
- startup and shutdown interest registration is deduped across grouped siblings
- the strategy reports stale-feed observations to the shared supervisor
- `on_quote_tick` records fresh timestamps with the supervisor
- strategies still receive quote ticks after the refactor and transition out of `bootstrapping/recovering` when fresh data arrives
- strategy snapshots continue to reflect honest quote state during `bootstrapping/blocked/recovering/down`
- required legs are only tradeable as a pair when feed state, session compatibility, and max leg-age skew all pass
- quote placement, quote amendment, and hedge placement are blocked while required feeds are non-tradeable
- non-tradeable required feeds force cancel-only behavior, pull working maker quotes, and still allow cancel/reduce-only or emergency-exit paths

**Step 2: Run the tests to confirm they fail**

Run: `./.venv.py312/bin/python -m pytest -q --import-mode=importlib tests/unit_tests/flux/strategies/equities_maker/test_strategy.py tests/unit_tests/flux/strategies/equities_taker/test_strategy.py -k 'quote_liveness or supervisor or resubscribe'`

Expected: FAIL because `MakerV4Strategy` still owns direct resubscribe side effects.

**Step 3: Implement the strategy integration**

Update `strategy.py` so:

- a shared supervisor can be configured during startup
- strategy-local quote-topic attachment/detachment is preserved explicitly for `on_quote_tick` delivery
- startup and shutdown register and deregister quote interest instead of owning external venue subscribe side effects
- the liveness timer becomes observer-only
- quote ticks report fresh timestamps into the supervisor
- direct external subscribe/unsubscribe/reset side effects are removed from strategy-owned logic
- quote-dependent execution paths gate on shared pair-level tradeability, not just stale display state
- on non-tradeable required-feed transitions, the strategy enters cancel-only mode, pulls working maker quotes, suppresses new quote/amend/hedge actions, and keeps cancel/reduce-only or emergency-exit paths enabled
- prolonged blocked/recovering states degrade through the existing component lifecycle rail and unrecoverable down/fatal conditions fault/escalate through the same rail

Do not remove the existing honest snapshot publication behavior.

**Step 4: Re-run the tests**

Run: `./.venv.py312/bin/python -m pytest -q --import-mode=importlib tests/unit_tests/flux/strategies/equities_maker/test_strategy.py tests/unit_tests/flux/strategies/equities_taker/test_strategy.py -k 'quote_liveness or supervisor or resubscribe'`

Expected: PASS

**Step 5: Run the broader strategy suites**

Run: `./.venv.py312/bin/python -m pytest -q --import-mode=importlib tests/unit_tests/flux/strategies/equities_maker/test_strategy.py`

Run: `./.venv.py312/bin/python -m pytest -q --import-mode=importlib tests/unit_tests/flux/strategies/equities_taker/test_strategy.py`

Expected: PASS

**Step 6: Commit**

```bash
git add systems/flux/flux/strategies/makerv4/strategy.py \
  tests/unit_tests/flux/strategies/equities_maker/test_strategy.py \
  tests/unit_tests/flux/strategies/equities_taker/test_strategy.py
git commit -m "refactor: move makerv4 quote recovery to shared supervisor"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Harden Hyperliquid Recovery Semantics

**Files:**
- Modify: `crates/adapters/hyperliquid/src/http/client.rs`
- Modify: `crates/adapters/hyperliquid/src/websocket/client.rs`
- Modify: `nautilus_trader/adapters/hyperliquid/data.py`
- Modify: `crates/adapters/hyperliquid/tests/http.rs`
- Modify: `crates/adapters/hyperliquid/tests/websocket.rs`
- Modify: `tests/integration_tests/adapters/hyperliquid/test_data.py`

**Step 1: Write the failing Hyperliquid recovery tests**

Add tests that prove:

- duplicate reset attempts for the same feed identity are idempotent
- cache-miss recovery is explicit and testable
- a reset path refreshes or validates instrument cache before quote subscribe
- a successful reset preserves desired quote-subscription state
- recovery tries desired-subscription replay on a healthy transport before escalating to reconnect/reset
- transport-local reset outcomes can be reported back to the shared supervisor without duplicating policy ownership

At least one Rust-layer test should model the `Instrument not found in cache` path that is currently appearing in live journals, because the warning is emitted below the thin Python adapter.

**Step 2: Run the tests to confirm they fail**

Run: `cargo test -p nautilus-hyperliquid --test http recovery -- --nocapture`

Run: `cargo test -p nautilus-hyperliquid --test websocket recovery -- --nocapture`

Run: `./.venv.py312/bin/python -m pytest -q tests/integration_tests/adapters/hyperliquid/test_data.py -k 'recovery or cache or subscribe_quotes'`

Expected: FAIL because the current client/adapter stack does not provide the required cache-aware recovery contract.

**Step 3: Implement Hyperliquid recovery hardening**

Update the Hyperliquid client/adapter path so:

- quote-reset behavior is idempotent
- cache preconditions are checked before re-subscribe
- cache-miss recovery becomes an explicit error or repair path, not warning-only churn
- desired-subscription replay on a healthy transport is attempted before reconnect/reset escalation
- reset state can be observed by the shared supervisor
- the adapter reports transport facts and explicit reset outcomes, but not lifecycle policy state
- transport outcomes re-enter the shared supervisor through the runner-owned ingress path rather than mutating Python supervisor state directly from async tasks

Do not introduce a new network daemon or change the external trading contract here.

**Step 4: Re-run the tests**

Run: `cargo test -p nautilus-hyperliquid --test http recovery -- --nocapture`

Run: `cargo test -p nautilus-hyperliquid --test websocket recovery -- --nocapture`

Run: `./.venv.py312/bin/python -m pytest -q tests/integration_tests/adapters/hyperliquid/test_data.py -k 'recovery or cache or subscribe_quotes'`

Expected: PASS

**Step 5: Commit**

```bash
git add nautilus_trader/adapters/hyperliquid/data.py \
  crates/adapters/hyperliquid/src/http/client.rs \
  crates/adapters/hyperliquid/src/websocket/client.rs \
  crates/adapters/hyperliquid/tests/http.rs \
  crates/adapters/hyperliquid/tests/websocket.rs \
  tests/integration_tests/adapters/hyperliquid/test_data.py
git commit -m "fix: harden hyperliquid quote recovery"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Harden Binance Recovery Semantics

**Files:**
- Modify: `nautilus_trader/adapters/binance/data.py`
- Modify: `nautilus_trader/adapters/binance/websocket/client.py`
- Create: `tests/unit_tests/adapters/binance/test_data_recovery.py`

**Step 1: Write the failing Binance recovery tests**

Add tests that prove:

- repeated grouped-node reset requests for one book-ticker subscription are idempotent
- desired subscription state remains explicit during reset windows
- a failed reset produces an explicit recovery failure instead of silent optimism
- websocket subscription state remains coherent across repeated reset requests
- recovery attempts desired-subscription replay on a healthy transport before escalating to reconnect/reset
- transport-local reset outcomes can be reported back to the shared supervisor without duplicating policy ownership

**Step 2: Run the tests to confirm they fail**

Run: `./.venv.py312/bin/python -m pytest -q tests/unit_tests/adapters/binance/test_data_recovery.py -k 'recovery or book_ticker or subscribe'`

Expected: FAIL because Binance quote reset behavior is still a thin pass-through.

**Step 3: Implement Binance recovery hardening**

Update `data.py` so:

- quote-reset behavior is idempotent
- duplicate sibling resets are collapsed safely
- desired-subscription replay is attempted before reconnect/reset escalation
- reset state can be surfaced to the shared supervisor

Update `websocket/client.py` as needed so the underlying subscription state remains coherent across repeated grouped-node resets.

Keep the change bounded to quote-feed recovery. Do not refactor unrelated Binance paths, do not move lifecycle policy ownership out of the shared supervisor, and do not mutate supervisor state directly from background websocket tasks.

**Step 4: Re-run the tests**

Run: `./.venv.py312/bin/python -m pytest -q tests/unit_tests/adapters/binance/test_data_recovery.py -k 'recovery or book_ticker or subscribe'`

Expected: PASS

**Step 5: Commit**

```bash
git add nautilus_trader/adapters/binance/data.py \
  nautilus_trader/adapters/binance/websocket/client.py \
  tests/unit_tests/adapters/binance/test_data_recovery.py
git commit -m "fix: harden binance quote recovery"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 5: Add Honest Recovering-State Semantics Without Regressing Down-State Behavior

**Files:**
- Modify: `systems/flux/flux/api/_payloads_signals.py`
- Modify: `tests/unit_tests/flux/api/test_payloads.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_readiness.py`
- Modify: `tests/unit_tests/flux/api/test_equities_profile_contract.py`

**Step 1: Write the failing honesty tests**

Add tests that prove:

- `bootstrapping`, `blocked`, and `recovering` feeds cannot serialize as `fresh`, `good`, or quote-usable
- public-facing payload fields keep the existing fail-closed contract rather than introducing new top-level enums
- readiness counts non-tradeable recovery/precondition states as unhealthy
- existing `down` semantics stay fail-closed and unchanged
- pair-level non-tradeable reasons remain explicit without leaking supervisor internals
- grouped-node or supervisor internals do not leak into external strategy ids

**Step 2: Run the tests to confirm they fail**

Run: `./.venv.py312/bin/python -m pytest -q tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/examples/strategies/test_equities_readiness.py tests/unit_tests/flux/api/test_equities_profile_contract.py -k 'recovering or down or quote_health'`

Expected: FAIL because the new internal non-tradeable supervisor states do not exist yet.

**Step 3: Implement the payload/readiness updates**

Update the payload and readiness builders so:

- `bootstrapping`, `blocked`, and `recovering` are added as internal fail-closed states
- existing `down` behavior remains intact
- stale/recovery reasons stay explicit
- pair-level disabled reasons remain explicit using the existing external contract vocabulary
- existing external payload contracts remain intact

**Step 4: Re-run the tests**

Run: `./.venv.py312/bin/python -m pytest -q tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/examples/strategies/test_equities_readiness.py tests/unit_tests/flux/api/test_equities_profile_contract.py -k 'recovering or down or quote_health'`

Expected: PASS

**Step 5: Commit**

```bash
git add systems/flux/flux/api/_payloads_signals.py \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/examples/strategies/test_equities_readiness.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py
git commit -m "fix: expose fail-closed recovery state for equities quotes"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 6: Validate Live Recovery And Cut The V1 Prod Release

**Files:**
- Modify: `docs/plans/2026-03-27-equities-shared-symbol-node.md`
- Modify: `docs/runbooks/equities-shared-node-cutover.md`
- Modify: `ops/scripts/deploy/check_equities_live_readiness.sh`

**Step 1: Capture the pre-rollout baseline and add any missing rollout assertions**

Before changing prod, capture:

- currently deployed equities release id / release root
- current `/etc/flux/equities*.env` targets and any systemd unit env-file pointers that must be restorable
- current readiness JSON
- current active market/session window for the targeted equities basket
- current quote-age probes for the known incident rows:
  - `aapl_tradexyz`
  - `amd_tradexyz`
  - `meta_tradexyz`
  - `msft_tradexyz`
  - `orcl_tradexyz`
  - `tsla_tradexyz`
  - `ewy_binance_perp`
- current IBKR auth and publisher health

Then extend readiness output if needed so rollout verification can prove:

- all `38` strategies are healthy
- no maker feed is stuck in `recovering/down`
- targeted historically stale symbols now advance quote timestamps
- strategies with non-tradeable required feeds hold zero working maker quotes
- the post-rollout result is strictly better than the captured pre-rollout baseline
- rollout failure can be diagnosed from structured recovery-state output

**Step 2: Run the full targeted verification suite**

Run:

- `./.venv.py312/bin/python -m pytest -q tests/unit_tests/examples/strategies/test_quote_feed_supervisor.py`
- `./.venv.py312/bin/python -m pytest -q --import-mode=importlib tests/unit_tests/flux/strategies/equities_maker/test_strategy.py`
- `./.venv.py312/bin/python -m pytest -q --import-mode=importlib tests/unit_tests/flux/strategies/equities_taker/test_strategy.py`
- `cargo test -p nautilus-hyperliquid --test http recovery -- --nocapture`
- `cargo test -p nautilus-hyperliquid --test websocket recovery -- --nocapture`
- `./.venv.py312/bin/python -m pytest -q tests/integration_tests/adapters/hyperliquid/test_data.py -k 'recovery or cache or subscribe_quotes'`
- `./.venv.py312/bin/python -m pytest -q tests/unit_tests/adapters/binance/test_data_recovery.py`
- `./.venv.py312/bin/python -m pytest -q tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/examples/strategies/test_equities_readiness.py tests/unit_tests/flux/api/test_equities_profile_contract.py`

Expected: PASS

**Step 3: Verify rollout preconditions**

Confirm before release cut:

- IBKR gateway is authenticated
- `chainsaw@md-ibkr-publisher.service` is healthy
- the current failure snapshot is recorded for comparison
- the currently deployed release id and `/etc/flux/equities*.env` targets are recorded in the rollout log so rollback can be executed without rediscovery
- if IBKR gateway or publisher health is bad, stop here and treat that as a shared-reference precondition failure rather than a maker-feed recovery result

Expected: PASS

**Step 4: Cut a fresh immutable equities release**

Run the standard release flow from the worktree and re-render `/etc/flux/equities*.env` against the release-local `.venv`.

**Step 5: Restart the equities stack and validate live**

Validate:

- public `/equities` and loopback/public API return `200`
- `bash ops/scripts/deploy/check_equities_live_readiness.sh --json` returns full health
- capture a pre-restart baseline of historically stale rows and compare post-restart results against it
- previously stale rows (`aapl_tradexyz`, `amd_tradexyz`, `meta_tradexyz`, `msft_tradexyz`, `orcl_tradexyz`, `tsla_tradexyz`, `ewy_binance_perp`) advance in repeated probes over a 10-15 minute soak
- rows do not claim `good` or `fresh` unless the quote timestamps are genuinely fresh
- supervisor state-transition logs show bounded recovery attempts instead of silent infinite loops
- during the restart/recovery window, any strategy whose required feeds are `bootstrapping`, `blocked`, `recovering`, or `down` emits no quote-placement, quote-amendment, or hedge-placement side effects and retains zero working maker quotes; prove this from structured strategy/order logs, counters, or venue state before declaring the rollout safe
- if a venue/session blocker trips, it is explicit in the logs and suppresses per-feed reset churn instead of leaving the node in silent retry storms
- final signoff sample is taken during a live US regular trading session, not only overnight
- rollback immediately if health regresses below baseline or the historically bad rows remain frozen after the soak window

**Step 6: Hold the release through a soak window**

Run repeated readiness and quote-age probes for a bounded soak window and confirm:

- the known bad rows stay live
- recovery logs show bounded attempts rather than churn
- strategies do not leak back into live maker quotes while required feeds remain non-tradeable
- no new grouped-node regression appears

**Step 7: Update the rollout tracker**

Record the exact release id, validation timestamps, remaining risks, and any rollback notes in `2026-03-27-equities-shared-symbol-node.md`.
Record the superseded release id and restorable `/etc/flux/equities*.env` targets alongside the new release id.
Update `docs/runbooks/equities-shared-node-cutover.md` so the operator procedure reflects the new supervisor-owned recovery model, the soak-window requirement, the fail-closed cancel-only / zero-working-quotes check during non-tradeable feed states, the explicit IBKR/publisher precondition boundary, and the rollback threshold against the pre-restart baseline.

**Step 8: Commit**

```bash
git add docs/plans/2026-03-27-equities-shared-symbol-node.md \
  docs/runbooks/equities-shared-node-cutover.md \
  ops/scripts/deploy/check_equities_live_readiness.sh
git commit -m "ops: validate equities market-data recovery v1 rollout"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

Plan complete and saved to `docs/plans/2026-03-28-equities-market-data-recovery-v1.md`. Two execution options:

**1. Subagent-Driven (this session)** - I dispatch fresh subagent per task, review between tasks, fast iteration

**2. Parallel Session (separate)** - Open new session with executing-plans, batch execution with checkpoints

**Which approach?**
