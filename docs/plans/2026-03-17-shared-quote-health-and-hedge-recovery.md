# Shared Quote Health And Hedge Recovery Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Unify market-data freshness semantics across Flux strategies and operator surfaces, then make missed-hedge handling explicit and recoverable so quiet books, stale quotes, and dropped feeds are distinguished cleanly without silently leaving unhedged risk.

**Architecture:** Introduce one shared quote-health evaluator used by strategies, readiness, API payload assembly, and UI contracts. The shared evaluator must distinguish transport/feed health from quote age and from actual trading eligibility. Makerv3 and Makerv4 both adopt the evaluator, while Makerv4 additionally adds a first-class hedge backlog/retry path so stale reference quotes fail closed but do not silently strand risk. Fluxboard and readiness then consume the same backend-authored semantics instead of inferring their own meanings from age fields.

**Tech Stack:** Python strategy/runtime code, Flux shared strategy modules, Redis-backed state exports, Flux API payload builders, equities readiness checks, Fluxboard React/TypeScript signal and alerts surfaces, pytest, vitest, live HL/IBKR operator semantics, existing Makerv3/Makerv4 alert infrastructure.

## Current Review Findings

1. The current codebase has three different effective definitions of "good market data":
   - Makerv3 blocks on stale maker or reference legs.
   - Makerv4 `take_take` validates only the IBKR leg before taking.
   - equities readiness treats over-age maker/reference legs as unhealthy even when Signal/API still says `tradeable=true`.
2. `quote age` is currently only a heuristic for "time since last observed update"; it does not tell operators whether the feed is broken, the book is just quiet, or the strategy should be blocked.
3. The AMD missed-hedge case indicates Makerv4 is failing closed on stale IBKR quotes, but without a first-class hedge backlog/retry contract and without a shared inventory/risk-delta reconciliation path.
4. Fluxboard and API currently overload operator concepts:
   - `tradeable` is not driven by the same rules the strategy uses.
   - old-but-connected quotes and true feed failure are not cleanly separated.
   - operators can see ages, but not an authoritative backend state explaining whether the quote is old, missing, or transport-bad.
5. TokenMM already has a stronger pattern for exposing age and quote-stale alerts in operator-facing contracts, so the remediation should align to a reusable cross-strategy contract instead of inventing a fourth one.
6. The user intent is explicit: preserve clean, maintainable, operator-friendly shared semantics that can be reused as more strategies and venues are added.

## Shared Contract To Implement

The shared quote-health module should evaluate each relevant leg and return:

- `feed_state`: `ok | degraded | down | unknown`
- `quote_state`: `fresh | old | missing`
- `quote_age_ms`
- `usable_for_pricing`
- `usable_for_hedging`
- `reason_code`
- `alert_level` when applicable

Operator-facing meaning:

- `feed bad` means transport/subscription health is bad or unknown, not merely that a quote has not changed recently.
- `quote old` means the last quote update is beyond the strategy's allowed freshness budget, even if the feed is still connected.
- `tradeable` means the strategy is allowed to put on new risk now.
- `hedgeable` means the strategy is allowed to send a hedge now.

Policy default:

- Old-but-connected quotes are **not** labeled as feed-stale.
- Old quotes should block new risk by default.
- Hedge submission on stale reference quotes remains fail-closed, but must create recoverable backlog state rather than silently dropping hedge intent.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | in_progress | main | none | `systems/flux/flux/strategies/shared`, `systems/flux/flux/strategies/makerv3`, `systems/flux/flux/strategies/makerv4`, `systems/flux/flux/api`, `systems/flux/flux/runners/equities`, `fluxboard`, `tests`, `docs/plans` | `shared` | `shared` | none | `pytest targeted slices PASS/FAIL recorded` | 2026-03-17 UTC shared evaluator landed; Makerv4 stale-maker gating and API quote-health enrichment are green in targeted slices; next focus is hedge backlog/retry and broader alignment |
| Task 1: Lock Shared Quote-Health Semantics In Tests And Docs | in_progress | main | none | `docs/plans/2026-03-17-shared-quote-health-and-hedge-recovery.md`, `tests/unit_tests/flux/api`, `tests/unit_tests/flux/strategies/makerv3`, `tests/unit_tests/flux/strategies/makerv4`, `fluxboard/docs` | `shared` | `shared` | none | `pytest red-phase FAIL then targeted PASS for test_quote_engine/test_strategy/test_equities_profile_contract` | 2026-03-17 UTC failing tests added and now passing for shared evaluator import, Makerv4 stale-maker block, and API quote-health fields; tokenmm contract doc follow-up still pending |
| Task 2: Implement Shared Quote-Health Evaluator Module | completed | main | Task 1: Lock Shared Quote-Health Semantics In Tests And Docs | `systems/flux/flux/strategies/shared/quote_health.py`, `tests/unit_tests/flux/strategies/shared` | `shared` | `shared` | none | `pytest tests/unit_tests/flux/strategies/shared/test_quote_health.py PASS` | 2026-03-17 UTC added pure shared evaluator with feed/quote state, usability flags, reason codes, and dual import-path aliasing |
| Task 3: Refactor MakerV3 To Use Shared Quote-Health Contracts | completed | Laplace | Task 2: Implement Shared Quote-Health Evaluator Module | `systems/flux/flux/strategies/makerv3`, `tests/unit_tests/flux/strategies/makerv3` | `shared` | `shared` | none | `pytest tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py PASS; stale lifecycle regression PASS` | 2026-03-17 UTC MakerV3 now routes refresh/timer freshness gates through the shared evaluator, preserves stale-leg block behavior, and publishes explicit per-leg quote_health fields/reason codes |
| Task 4: Refactor MakerV4 Pricing And Hedge Gates Onto Shared Quote Health | in_progress | main | Task 2: Implement Shared Quote-Health Evaluator Module | `systems/flux/flux/strategies/makerv4`, `tests/unit_tests/flux/strategies/makerv4` | `shared` | `shared` | none | `pytest targeted Makerv4/API slice PASS` | 2026-03-17 UTC Makerv4 quote snapshots now expose leg-level quote health and take_take blocks old maker quotes in the targeted test; stale-reference hedge lifecycle still needs backlog/retry work |
| Task 5: Add Makerv4 Hedge Backlog, Retry, And Inventory Reconciliation | in_progress | main | Task 4: Refactor MakerV4 Pricing And Hedge Gates Onto Shared Quote Health | `systems/flux/flux/strategies/makerv4`, `tests/unit_tests/flux/strategies/makerv4`, `tests/unit_tests/flux/common` | `shared` | `shared` | none | `pytest focused Makerv4 hedge backlog slice PASS/FAIL recorded` | 2026-03-17 UTC main lane owns stale-reference hedge backlog/retry and risk projection reconciliation |
| Task 6: Align API Payloads And Readiness On Shared Semantics | in_progress | Nash + main | Task 3: Refactor MakerV3 To Use Shared Quote-Health Contracts, Task 4: Refactor MakerV4 Pricing And Hedge Gates Onto Shared Quote Health | `systems/flux/flux/api`, `systems/flux/flux/runners/equities/readiness.py`, `tests/unit_tests/flux/api`, `tests/unit_tests/examples/strategies` | `shared` | `shared` | none | `pytest test_equities_profile_contract and test_equities_readiness PASS/FAIL recorded` | 2026-03-17 UTC `_payloads_signals.py` enriches MakerV4 quote legs with feed/quote state using shared thresholds; readiness alignment delegated to Nash while API completion stays on main |
| Task 7: Update Fluxboard Signal And Alerts UX For Feed State, Quote Age, And Trading Eligibility | not_started | unassigned | Task 6: Align API Payloads And Readiness On Shared Semantics | `fluxboard/components/domain/signal`, `fluxboard/utils`, `fluxboard/types.ts`, `fluxboard/tests/signal`, `fluxboard/docs` | `shared` | `shared` | none | not_run | Plan created |
| Task 8: Run End-To-End Verification And Live Safety Acceptance | not_started | unassigned | Task 5: Add Makerv4 Hedge Backlog, Retry, And Inventory Reconciliation, Task 6: Align API Payloads And Readiness On Shared Semantics, Task 7: Update Fluxboard Signal And Alerts UX For Feed State, Quote Age, And Trading Eligibility | `tests`, `ops/scripts`, `docs/plans/2026-03-17-shared-quote-health-and-hedge-recovery.md` | `shared` | `shared` | none | not_run | Plan created |

---

### Task 1: Lock Shared Quote-Health Semantics In Tests And Docs

**Files:**
- Modify: `docs/plans/2026-03-17-shared-quote-health-and-hedge-recovery.md`
- Modify: `fluxboard/docs/tokenmm_contract.md`
- Modify: `tests/unit_tests/flux/api/test_equities_profile_contract.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`

**Dependencies:** `none`

**Write Scope:** `docs/plans/2026-03-17-shared-quote-health-and-hedge-recovery.md`, `fluxboard/docs/tokenmm_contract.md`, `tests/unit_tests/flux/api/test_equities_profile_contract.py`, `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`, `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`

**Verification Commands:**
- `./.venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/strategies/makerv4/test_strategy.py tests/unit_tests/flux/api/test_equities_profile_contract.py -k 'quote_health or stale_quote or tradeable or hedge_disabled_reason' -p no:rerunfailures`

**Step 1: Write the failing tests**

Add tests that pin the shared semantic contract:

- old quote with healthy transport is `quote_state=old`, not `feed_state=down`
- missing quote is `quote_state=missing`
- dropped transport or unknown transport is `feed_state != ok`
- old quote blocks new risk by default
- stale reference quote after maker fill creates a hedge-block condition rather than silently pretending success
- API payloads expose explicit backend-authored fields for feed and quote state instead of relying on UI heuristics alone

Example test shape:

```python
def test_quote_old_is_not_feed_down():
    health = evaluate_quote_health(
        leg_role="maker",
        bid=Decimal("100"),
        ask=Decimal("101"),
        quote_age_ms=12_000,
        max_quote_age_ms=10_000,
        transport_connected=True,
        subscription_healthy=True,
    )

    assert health.feed_state == "ok"
    assert health.quote_state == "old"
    assert health.usable_for_pricing is False
```

**Step 2: Run tests to verify they fail**

Run: `./.venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/strategies/makerv4/test_strategy.py tests/unit_tests/flux/api/test_equities_profile_contract.py -k 'quote_health or stale_quote or tradeable or hedge_disabled_reason' -p no:rerunfailures`

Expected: FAIL because the shared contract and fields do not exist yet.

**Step 3: Update docs to record the intended semantics**

Document the operator contract in `fluxboard/docs/tokenmm_contract.md` and this plan:

- `quote age` is informational
- `feed_state` reports transport health
- `quote_state` reports freshness/presence
- `tradeable` and `hedgeable` come from backend policy, not UI inference

**Step 4: Re-run the focused test slice after Tasks 2-6**

Expected: PASS.

**Step 5: Commit**

```bash
git add docs/plans/2026-03-17-shared-quote-health-and-hedge-recovery.md \
  fluxboard/docs/tokenmm_contract.md \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py
git commit -m "test: lock shared quote health semantics"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Implement Shared Quote-Health Evaluator Module

**Files:**
- Create: `systems/flux/flux/strategies/shared/quote_health.py`
- Create: `tests/unit_tests/flux/strategies/shared/test_quote_health.py`
- Modify: `systems/flux/flux/strategies/shared/__init__.py` if needed

**Dependencies:** `Task 1: Lock Shared Quote-Health Semantics In Tests And Docs`

**Write Scope:** `systems/flux/flux/strategies/shared/quote_health.py`, `tests/unit_tests/flux/strategies/shared/test_quote_health.py`, `systems/flux/flux/strategies/shared/__init__.py`

**Verification Commands:**
- `./.venv/bin/pytest -q tests/unit_tests/flux/strategies/shared/test_quote_health.py -p no:rerunfailures`

**Step 1: Write the failing shared-module tests**

Cover:

- `fresh`
- `old`
- `missing`
- `feed degraded/down`
- separate `usable_for_pricing` and `usable_for_hedging`
- canonical `reason_code` values

**Step 2: Run test to verify it fails**

Run: `./.venv/bin/pytest -q tests/unit_tests/flux/strategies/shared/test_quote_health.py -p no:rerunfailures`

Expected: FAIL because the module does not exist.

**Step 3: Write minimal implementation**

Create a small shared evaluator with typed result objects or dicts that accept:

- leg role
- bid/ask presence
- quote age
- freshness threshold
- optional transport/subscription health flags

Return:

- `feed_state`
- `quote_state`
- `usable_for_pricing`
- `usable_for_hedging`
- `reason_code`
- `alert_level`

Keep the initial implementation pure and side-effect free so strategies, readiness, and API can all reuse it.

**Step 4: Run test to verify it passes**

Run: `./.venv/bin/pytest -q tests/unit_tests/flux/strategies/shared/test_quote_health.py -p no:rerunfailures`

Expected: PASS.

**Step 5: Commit**

```bash
git add systems/flux/flux/strategies/shared/quote_health.py \
  tests/unit_tests/flux/strategies/shared/test_quote_health.py \
  systems/flux/flux/strategies/shared/__init__.py
git commit -m "feat: add shared quote health evaluator"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Refactor MakerV3 To Use Shared Quote-Health Contracts

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/quote_engine.py`
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv3/publisher.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`

**Dependencies:** `Task 2: Implement Shared Quote-Health Evaluator Module`

**Write Scope:** `systems/flux/flux/strategies/makerv3/quote_engine.py`, `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv3/publisher.py`, `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`

**Verification Commands:**
- `./.venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -p no:rerunfailures`

**Step 1: Write the failing regression tests**

Require MakerV3 to:

- keep existing block behavior on stale maker/reference legs
- publish explicit quote-health fields/reason codes
- distinguish feed problems from quote-old conditions

**Step 2: Run tests to verify failure**

Run: `./.venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -p no:rerunfailures`

Expected: FAIL because MakerV3 does not use the shared evaluator yet.

**Step 3: Refactor MakerV3 onto the shared evaluator**

Replace duplicated age checks in `quote_engine.py` with calls to `quote_health.py`. Preserve current risk behavior:

- stale maker quote blocks and cancels managed orders
- stale reference quote blocks and cancels managed orders

Add explicit exported state fields that API/UI can reuse.

**Step 4: Run tests to verify pass and no regression**

Run: `./.venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -p no:rerunfailures`

Expected: PASS.

**Step 5: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/quote_engine.py \
  systems/flux/flux/strategies/makerv3/strategy.py \
  systems/flux/flux/strategies/makerv3/publisher.py \
  tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py
git commit -m "refactor: move makerv3 quote health to shared evaluator"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Refactor MakerV4 Pricing And Hedge Gates Onto Shared Quote Health

**Files:**
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv4/pricing.py`
- Modify: `systems/flux/flux/strategies/makerv4/publisher.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_pricing.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py`

**Dependencies:** `Task 2: Implement Shared Quote-Health Evaluator Module`

**Write Scope:** `systems/flux/flux/strategies/makerv4/strategy.py`, `systems/flux/flux/strategies/makerv4/pricing.py`, `systems/flux/flux/strategies/makerv4/publisher.py`, `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`, `tests/unit_tests/flux/flux/strategies/makerv4/test_pricing.py`, `tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py`

**Verification Commands:**
- `./.venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv4/test_strategy.py tests/unit_tests/flux/strategies/makerv4/test_pricing.py tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py -k 'quote_health or take_take or stale_quote or blocked_stale_quote or hedge_disabled_reason' -p no:rerunfailures`

**Step 1: Write the failing tests**

Require Makerv4 to:

- block `take_take` when the maker leg is old or missing, not just when IBKR is stale
- expose separate maker-leg and reference-leg health in the published quote snapshot/operator payload
- stop setting `tradeable=true` when backend freshness policy says new risk is unsafe

**Step 2: Run tests to verify failure**

Run: `./.venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv4/test_strategy.py tests/unit_tests/flux/strategies/makerv4/test_pricing.py tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py -k 'quote_health or take_take or stale_quote or blocked_stale_quote or hedge_disabled_reason' -p no:rerunfailures`

Expected: FAIL because Makerv4 currently validates only the reference quote on the take path.

**Step 3: Implement the shared gating**

Refactor:

- maker quote / take pricing to use shared maker-leg health
- hedge submission to use shared reference-leg health
- state publishing to expose both leg-health summaries

Preserve current fail-closed behavior on stale reference quotes at hedge time.

**Step 4: Run tests to verify pass**

Run: `./.venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv4/test_strategy.py tests/unit_tests/flux/strategies/makerv4/test_pricing.py tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py -k 'quote_health or take_take or stale_quote or blocked_stale_quote or hedge_disabled_reason' -p no:rerunfailures`

Expected: PASS.

**Step 5: Commit**

```bash
git add systems/flux/flux/strategies/makerv4/strategy.py \
  systems/flux/flux/strategies/makerv4/pricing.py \
  systems/flux/flux/strategies/makerv4/publisher.py \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv4/test_pricing.py \
  tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py
git commit -m "refactor: unify makerv4 quote health gating"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 5: Add Makerv4 Hedge Backlog, Retry, And Inventory Reconciliation

**Files:**
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv4/managed_orders.py`
- Modify: `systems/flux/flux/strategies/makerv4/publisher.py`
- Modify: `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`
- Modify: `tests/unit_tests/flux/common/test_account_projection.py`

**Dependencies:** `Task 4: Refactor MakerV4 Pricing And Hedge Gates Onto Shared Quote Health`

**Write Scope:** `systems/flux/flux/strategies/makerv4/strategy.py`, `systems/flux/flux/strategies/makerv4/managed_orders.py`, `systems/flux/flux/strategies/makerv4/publisher.py`, `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`, `tests/unit_tests/flux/common/test_account_projection.py`

**Verification Commands:**
- `./.venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv4/test_strategy.py tests/unit_tests/flux/common/test_account_projection.py -k 'pending_hedge or backlog or stale_quote or risk_delta or retry' -p no:rerunfailures`

**Step 1: Write failing tests for missed-hedge recovery**

Pin behavior for:

- maker/taker fill arrives while reference quote is old
- no hedge order is sent immediately
- strategy opens a recoverable hedge backlog entry
- new risk is blocked until backlog clears
- fresh reference quote later triggers hedge retry
- `risk_delta` and operator payload reflect the open unhedged exposure

**Step 2: Run tests to verify failure**

Run: `./.venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv4/test_strategy.py tests/unit_tests/flux/common/test_account_projection.py -k 'pending_hedge or backlog or stale_quote or risk_delta or retry' -p no:rerunfailures`

Expected: FAIL because current stale-quote hedge failure just disables hedging and returns.

**Step 3: Implement backlog and retry**

Add:

- explicit hedge backlog state
- retry on fresh IBKR quote or next allowed decision point
- clear operator reasons and alert codes
- freeze new risk while backlog is open
- consistent inventory/risk projection updates

Avoid introducing a general-purpose workflow engine. Keep the retry contract minimal and deterministic.

**Step 4: Run tests to verify pass**

Run: `./.venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv4/test_strategy.py tests/unit_tests/flux/common/test_account_projection.py -k 'pending_hedge or backlog or stale_quote or risk_delta or retry' -p no:rerunfailures`

Expected: PASS.

**Step 5: Commit**

```bash
git add systems/flux/flux/strategies/makerv4/strategy.py \
  systems/flux/flux/strategies/makerv4/managed_orders.py \
  systems/flux/flux/strategies/makerv4/publisher.py \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/unit_tests/flux/common/test_account_projection.py
git commit -m "feat: add makerv4 hedge backlog recovery"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 6: Align API Payloads And Readiness On Shared Semantics

**Files:**
- Modify: `systems/flux/flux/api/_payloads_signals.py`
- Modify: `systems/flux/flux/api/payloads.py`
- Modify: `systems/flux/flux/runners/equities/readiness.py`
- Modify: `tests/unit_tests/flux/api/test_equities_profile_contract.py`
- Modify: `tests/unit_tests/flux/api/test_payloads.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_readiness.py`

**Dependencies:** `Task 3: Refactor MakerV3 To Use Shared Quote-Health Contracts`, `Task 4: Refactor MakerV4 Pricing And Hedge Gates Onto Shared Quote Health`

**Write Scope:** `systems/flux/flux/api/_payloads_signals.py`, `systems/flux/flux/api/payloads.py`, `systems/flux/flux/runners/equities/readiness.py`, `tests/unit_tests/flux/api/test_equities_profile_contract.py`, `tests/unit_tests/flux/api/test_payloads.py`, `tests/unit_tests/examples/strategies/test_equities_readiness.py`

**Verification Commands:**
- `./.venv/bin/pytest -q tests/unit_tests/flux/api/test_equities_profile_contract.py tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/examples/strategies/test_equities_readiness.py -k 'tradeable or quote_state or feed_state or over_age or stale_signal' -p no:rerunfailures`

**Step 1: Write failing API/readiness tests**

Require:

- `/signals` exposes explicit per-leg or per-strategy quote-health fields
- `tradeable` comes from backend freshness policy, not from a shortcut based only on `bot_on` and blocked state
- readiness and API classify the same legs unhealthy under the same conditions

**Step 2: Run tests to verify failure**

Run: `./.venv/bin/pytest -q tests/unit_tests/flux/api/test_equities_profile_contract.py tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/examples/strategies/test_equities_readiness.py -k 'tradeable or quote_state or feed_state or over_age or stale_signal' -p no:rerunfailures`

Expected: FAIL because API and readiness currently use parallel, divergent logic.

**Step 3: Implement alignment**

Make readiness consume the shared evaluator or the same canonical rules as the strategy export. Add explicit payload fields needed by UI and ops:

- `feed_state`
- `quote_state`
- `pricing_usable`
- `hedgeable`
- `reason_code`

Do not recalculate strategy policy independently inside Fluxboard.

**Step 4: Run tests to verify pass**

Run: `./.venv/bin/pytest -q tests/unit_tests/flux/api/test_equities_profile_contract.py tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/examples/strategies/test_equities_readiness.py -k 'tradeable or quote_state or feed_state or over_age or stale_signal' -p no:rerunfailures`

Expected: PASS.

**Step 5: Commit**

```bash
git add systems/flux/flux/api/_payloads_signals.py \
  systems/flux/flux/api/payloads.py \
  systems/flux/flux/runners/equities/readiness.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/examples/strategies/test_equities_readiness.py
git commit -m "refactor: align api and readiness on quote health"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 7: Update Fluxboard Signal And Alerts UX For Feed State, Quote Age, And Trading Eligibility

**Files:**
- Modify: `fluxboard/components/domain/signal/SignalTable.tsx`
- Modify: `fluxboard/components/domain/signal/MakerV4SignalTable.tsx`
- Modify: `fluxboard/types.ts`
- Modify: `fluxboard/utils/age.ts`
- Modify: `fluxboard/tests/signal/MakerV4SignalTable.test.tsx`
- Modify: `fluxboard/tests/signal/SignalTable.audit.test.tsx`
- Modify: `fluxboard/docs/equities_contract.md`

**Dependencies:** `Task 6: Align API Payloads And Readiness On Shared Semantics`

**Write Scope:** `fluxboard/components/domain/signal/SignalTable.tsx`, `fluxboard/components/domain/signal/MakerV4SignalTable.tsx`, `fluxboard/types.ts`, `fluxboard/utils/age.ts`, `fluxboard/tests/signal/MakerV4SignalTable.test.tsx`, `fluxboard/tests/signal/SignalTable.audit.test.tsx`, `fluxboard/docs/equities_contract.md`

**Verification Commands:**
- `pnpm --dir fluxboard exec vitest run tests/signal/MakerV4SignalTable.test.tsx tests/signal/SignalTable.audit.test.tsx`

**Step 1: Write failing UI tests**

Require the Signal page to:

- still show age clearly
- show bad market-data state separately from age
- stop implying that every old quote means dead feed
- reflect backend-authored `tradeable` / `hedgeable` rather than recomputing those semantics locally

**Step 2: Run tests to verify failure**

Run: `pnpm --dir fluxboard exec vitest run tests/signal/MakerV4SignalTable.test.tsx tests/signal/SignalTable.audit.test.tsx`

Expected: FAIL because the UI does not yet expose the new contract.

**Step 3: Implement minimal UX alignment**

Add clear but compact operator-facing rendering:

- age remains visible
- feed/quote health reasons appear in tooltip or badges
- `Trading`/`Ready` semantics reflect backend policy
- do not overload `Pending`/`Blocked` with freshness-only meaning

Prefer simple labels over dense abstractions. Operators should be able to tell at a glance:

- feed broken
- quote old
- new risk blocked
- hedge backlog open

**Step 4: Run tests to verify pass**

Run: `pnpm --dir fluxboard exec vitest run tests/signal/MakerV4SignalTable.test.tsx tests/signal/SignalTable.audit.test.tsx`

Expected: PASS.

**Step 5: Commit**

```bash
git add fluxboard/components/domain/signal/SignalTable.tsx \
  fluxboard/components/domain/signal/MakerV4SignalTable.tsx \
  fluxboard/types.ts \
  fluxboard/utils/age.ts \
  fluxboard/tests/signal/MakerV4SignalTable.test.tsx \
  fluxboard/tests/signal/SignalTable.audit.test.tsx \
  fluxboard/docs/equities_contract.md
git commit -m "feat: surface shared quote health in fluxboard"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 8: Run End-To-End Verification And Live Safety Acceptance

**Files:**
- Modify: `docs/plans/2026-03-17-shared-quote-health-and-hedge-recovery.md`
- Modify: any touched tests if verification reveals gaps

**Dependencies:** `Task 5: Add Makerv4 Hedge Backlog, Retry, And Inventory Reconciliation`, `Task 6: Align API Payloads And Readiness On Shared Semantics`, `Task 7: Update Fluxboard Signal And Alerts UX For Feed State, Quote Age, And Trading Eligibility`

**Write Scope:** `docs/plans/2026-03-17-shared-quote-health-and-hedge-recovery.md`, already-touched test files only if fixes are required

**Verification Commands:**
- `./.venv/bin/pytest -q tests/unit_tests/flux/strategies/shared/test_quote_health.py tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py tests/unit_tests/flux/strategies/makerv4/test_strategy.py tests/unit_tests/flux/strategies/makerv4/test_pricing.py tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py tests/unit_tests/flux/api/test_equities_profile_contract.py tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/examples/strategies/test_equities_readiness.py tests/unit_tests/flux/common/test_account_projection.py -p no:rerunfailures`
- `pnpm --dir fluxboard exec vitest run tests/signal/MakerV4SignalTable.test.tsx tests/signal/SignalTable.audit.test.tsx`
- `git diff --check`

**Step 1: Run the full focused verification suite**

Execute all strategy/API/UI slices touched by the remediation.

**Step 2: Fix any last failing tests without widening scope**

Only patch files already in scope. Do not start follow-on refactors here.

**Step 3: Perform live acceptance checks**

Check the live stack against the contract:

- old-but-connected quotes show `quote_state=old` not `feed down`
- dropped feed or broken subscription produces feed-bad state
- stale reference quote after fill opens hedge backlog and blocks new risk
- fresh reference quote clears backlog after hedge submit/fill
- `/equities/alerts` surfaces the stale-hedge and backlog transitions

**Step 4: Update the Progress Tracker and notes**

Record:

- verification commands and results
- remaining operational caveats
- live follow-ups if any remain

**Step 5: Commit**

```bash
git add docs/plans/2026-03-17-shared-quote-health-and-hedge-recovery.md
git commit -m "docs: close shared quote health remediation plan"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.
