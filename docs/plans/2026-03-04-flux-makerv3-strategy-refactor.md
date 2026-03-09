# Flux MakerV3 Strategy Refactor Implementation Plan (Strategy-only)

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
>
> **For executing agent (this repo):** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task (fresh implementer subagent per task, then spec review, then code-quality review).

**Goal:** Productionize the `makerv3` quoting strategy currently implemented as a monolith in `nautilus_trader/flux/strategies/makerv3/single_leg_quoter.py` by making it modular, safe (order lifecycle + cancellation), config-driven, and operationally observable. Standardize naming so the canonical strategy is `makerv3` (not `single_leg_quoter`) and remove legacy `poc` naming from production interfaces.

**Architecture:** Thin `MakerV3Strategy` orchestrator plus pure modules for pricing/ladder math, rebalancing planning, managed-order reconciliation, runtime params registry, and wire/event payloads. Lock behavior with invariants tests first, then perform a staged refactor with compatibility shims to avoid breaking imports/topics.

**Tech Stack:** Nautilus Trader Strategy API, Python, Nautilus MessageBus, Redis-backed Flux params subsystem, Pytest.

---

## Scope / Non-goals

**In scope**

1. Strategy core logic, safety, performance, and production observability.
2. Runtime params wiring and schema consistency for this strategy (including API/params manager alignment).
3. Naming standardization to `flux` + `makerv3` surfaces (modules/classes/topics/payload types).
4. Strategy-specific docs for architecture, invariants, and ops playbook.

**Out of scope**

1. Fluxboard/UI work.
2. Adding new venues/feature expansions beyond the current behavior (this plan only hardens and modularizes what exists).

---

## Key files (current)

1. Strategy: `nautilus_trader/flux/strategies/makerv3/single_leg_quoter.py`
2. Example strategy copy (parity/duplication risk): `nautilus_trader/examples/strategies/makerv3_single_leg_quoter.py`
3. Strategy tests: `tests/unit_tests/flux/strategies/makerv3/test_single_leg_quoter_math.py`, `tests/unit_tests/flux/strategies/makerv3/test_single_leg_quoter_strategy.py`
4. Example tests: `tests/unit_tests/examples/strategies/test_makerv3_single_leg_quoter.py`
5. Params manager: `nautilus_trader/flux/params/manager.py`
6. Key builders/config: `nautilus_trader/flux/common/keys.py`, `nautilus_trader/flux/common/config.py`
7. Logging guidance: `docs/concepts/logging.md`

---

## Production bar (acceptance criteria)

1. No legacy `poc` naming in production module paths, strategy IDs, topic prefixes, payload type strings, docs, or defaults.
2. No hardcoded instruments/venues/products/strategy names in production modules (only in example wrappers).
3. Strategy cancels and reconciles only its own managed orders by default; no unsafe “cancel everything” behavior.
4. Order lifecycle reconciliation is deterministic and idempotent across fills/cancels/rejects/expires, including restart/reconnect windows.
5. Hot path (`on_order_book_deltas`) is allocation-light and avoids avoidable work: no repeated parsing/coercion, no repeated cache scans, no spam logging.
6. Quote churn is bounded: recompute only when quote inputs change and within throttle; stale-data behavior does not cause repeated cancel bursts.
7. Runtime params are actually effective in live runs: params manager is wired, schema is canonical, and runtime updates are validated and bounded.
8. Observability: quote cycle events, state transitions, and cancellation outcomes are structured and rate-limited; alerts are emitted only for actionable conditions.
9. Tests cover key invariants (depth caps, improve-only, cancel/replace rules, stale gating, params updates, and cancellation safety).

---

## Guardrails (keep execution painless)

1. Prefer behavior-preserving refactors: add tests first and do moves/extractions in small steps.
2. Do not “clean up” formatting or rename unrelated symbols opportunistically; only change what reduces production risk.
3. Keep the hot path free of external I/O and rate-limited on noisy logs/alerts.
4. Maintain a compatibility story for imports and topic names until downstream consumers are updated.

---

## Review findings (severity ordered)

### P0 (production blocking)

1. Unsafe cancel boundary: `_cancel_managed_quotes` cancels strategy-managed orders, then unconditionally calls `cancel_all_orders(instrument)` which can cancel unrelated orders on the same instrument. This violates safety boundaries and can cause cross-strategy impact. See `nautilus_trader/flux/strategies/makerv3/single_leg_quoter.py:1789`.
2. Runtime params manager appears not to be wired by any in-repo callsite. `set_params_manager(...)` exists but strategy may never receive a manager instance, making live runtime param updates a no-op. See `nautilus_trader/flux/strategies/makerv3/single_leg_quoter.py:1011`.

### P1 (high risk / high leverage)

1. Monolithic strategy file (~2000 LOC) mixes quoting math, params, inventory, serialization, lifecycle, reconciliation, and event publishing; it is not maintainable at production trading quality. See `nautilus_trader/flux/strategies/makerv3/single_leg_quoter.py:3`.
2. Identity drift risk: Redis keyspace identity and published payload “strategy_id” appear to be different fields (`strategy_id` vs `_external_strategy_id`), creating fragmented control/visibility and incorrect operator mental model. See `nautilus_trader/flux/strategies/makerv3/single_leg_quoter.py:800`, `nautilus_trader/flux/common/keys.py:45`.
3. Stale data handling can trigger repeated cancel bursts because early-return paths do not update quote-throttle state. See `nautilus_trader/flux/strategies/makerv3/single_leg_quoter.py:1392`.

### P2 (important hardening)

1. Fill reconciliation depends on cache timing (`on_order_filled` only reconciles when cache order is closed), risking transient stale tracking and incorrect quote-stack decisions. See `nautilus_trader/flux/strategies/makerv3/single_leg_quoter.py:1169`.
2. Runtime quote depth is effectively unbounded via runtime `n_orders*` (non-negative only). A bad update can blow up CPU and allocations on hot path. See `nautilus_trader/flux/strategies/makerv3/single_leg_quoter.py:122`.
3. Topic naming is too generic (`flux.strategy.*`) for a specific strategy family and makes long-term evolution/versioning harder. See `nautilus_trader/flux/strategies/makerv3/single_leg_quoter.py:12`.
4. Hot-path performance issues: per-delta string conversions and repeated runtime param coercion; expensive inventory skew recomputation each refresh; repeated cache scans in `_managed_orders`. See `nautilus_trader/flux/strategies/makerv3/single_leg_quoter.py:931`, `nautilus_trader/flux/strategies/makerv3/single_leg_quoter.py:1299`, `nautilus_trader/flux/strategies/makerv3/single_leg_quoter.py:1740`.
5. Observability gaps: skipped quote cycles and cancellation outcomes are under-instrumented; stale-data warnings can spam. See `nautilus_trader/flux/strategies/makerv3/single_leg_quoter.py:931`, `nautilus_trader/flux/strategies/makerv3/single_leg_quoter.py:1789`, and `docs/concepts/logging.md`.

### P3 (cleanup / maintainability)

1. Legacy payload type name `MakerPocBusPayload` appears in production strategy code; should be replaced with a `makerv3`-scoped payload schema. See `nautilus_trader/flux/strategies/makerv3/single_leg_quoter.py:30`.
2. Tests depend directly on underscored helpers in the strategy module, making refactors unnecessarily painful. See `tests/unit_tests/flux/strategies/makerv3/test_single_leg_quoter_math.py:20`.

---

## Target module layout (proposed)

Goal: make `single_leg_quoter.py` disappear from canonical imports; retain a temporary compatibility shim only if needed.

New/updated files under `nautilus_trader/flux/strategies/makerv3/`:

1. `constants.py`:
   - Topic names, timer names, interval constants.
2. `types.py`:
   - `MakerV3StrategyConfig`, `MakerV3RuntimeParams`, `MakerV3BusPayload` schema types.
3. `pricing.py`:
   - Pure math helpers: tick rounding, post-only clamp, ladder builders, basis-point helpers.
4. `rebalancing.py`:
   - `plan_side_rebalance_actions` and related pure reconciliation planning helpers.
5. `inventory.py`:
   - Inventory/position/balance extraction and skew computation (with caching hooks).
6. `managed_orders.py`:
   - `ManagedOrderSet` abstraction and cancellation/reconcile helpers (strategy-scoped safety).
7. `wire.py`:
   - JSON serializers and msgbus event payload builders for state/event/trade/alert/bbo/fv/balances.
8. `strategy.py`:
   - `MakerV3Strategy` orchestration (lifecycle hooks, hot-path gating, calls into the pure modules).
9. `__init__.py`:
   - Export canonical strategy/config names.

Optional shared modules under `nautilus_trader/flux/common/`:

1. `params.py`:
   - `RuntimeParamSpec` + `RuntimeParamRegistry` so API + strategy share one canonical schema/defaults/constraints.

---

## Naming and compatibility plan

**Canonical naming (target)**

1. Strategy class: `MakerV3Strategy`
2. Strategy config: `MakerV3StrategyConfig`
3. Payload schema: `MakerV3BusPayload`
4. Topics: `flux.makerv3.*` (e.g., `flux.makerv3.state`, `flux.makerv3.event`, `flux.makerv3.alert`)

**Compatibility (recommended to reduce breakage)**

1. Keep `nautilus_trader/flux/strategies/makerv3/single_leg_quoter.py` temporarily as:
   - a thin re-export shim with deprecation warning; no logic.
2. Publish to both topic namespaces during a transition window:
   - publish to `flux.makerv3.*` and (optionally) also to existing `flux.strategy.*` until consumers migrate.
3. Keep old class/config names as aliases (deprecated) to avoid breaking existing imports/tests; remove in a later cleanup pass.

---

## HFT safety invariants (must hold)

1. Strategy must never cancel or modify orders not owned by this strategy instance by default.
2. Managed order depth on each side must be bounded by a validated maximum and must not increase without explicit config change.
3. Quote cycles must be idempotent: repeated market deltas with unchanged inputs must not generate repeated cancel/replace churn.
4. Stale data must trigger a single safety cancel + “blocked” state transition, not repeated cancel bursts at throttle cadence.
5. `on_stop` must cancel all managed orders and converge to quiescence deterministically (idempotent; observable).
6. Runtime param updates must be validated and bounded; unsafe updates must be rejected (and alert) without destabilizing the hot path.

---

## Status tracking (executing agent checklist)

Phase 0: Plan execution setup

- [ ] Confirm worktree path and branch
- [ ] Confirm pytest targets and baseline run time budget
- [ ] Capture “behavior baseline” notes for quoting/cancel rules

Phase 1: Safety blockers

- [ ] Remove unsafe cancel-all behavior (strategy-owned cancel only)
- [ ] Fix stale cancel-burst behavior (dedupe + throttle state update)
- [ ] Fix fill reconciliation/tracking determinism

Phase 2: Runtime params correctness

- [ ] Ensure params manager is actually wired in live runner
- [ ] Unify param schema/defaults/constraints across strategy + API
- [ ] Add bounds for depth/ladder params (HFT-safe guards)

Phase 3: Observability and alerts

- [ ] Introduce quote-cycle IDs and reason codes
- [ ] Add state transition events and cancel outcome counters
- [ ] Rate-limit noisy warnings/events on hot path

Phase 4: Modularization + rename

- [ ] Extract pricing helpers into `pricing.py`
- [ ] Extract rebalancing planner into `rebalancing.py`
- [ ] Extract inventory/skew into `inventory.py`
- [ ] Extract managed order tracking into `managed_orders.py`
- [ ] Extract wire/payload builders into `wire.py`
- [ ] Introduce `strategy.py` and migrate canonical class naming
- [ ] Convert `single_leg_quoter.py` into compatibility shim (or remove once consumers updated)
- [ ] Convert example strategy file into a thin wrapper (no duplicated logic)

Phase 5: Tests and docs

- [ ] Expand unit tests around invariants (idempotency, staleness boundaries, cancel safety, params semantics)
- [ ] Add docs: strategy architecture, invariants, and ops playbook

---

## Execution plan (task-by-task)

Note: each task is intentionally small. Execute with TDD where feasible and prefer pure functions for testability.

### Task 1: Lock current invariants with tests (before refactor)

**Files:**

- Modify: `tests/unit_tests/flux/strategies/makerv3/test_single_leg_quoter_strategy.py`
- Modify: `tests/unit_tests/examples/strategies/test_makerv3_single_leg_quoter.py`

**Steps:**

1. Add a unit test asserting staleness boundary condition (`age_ms == max_age_ms` is stale).
2. Add a unit test covering runtime param refresh idempotency and “no-op when unchanged”.
3. Add a unit test covering `_cancel_managed_quotes` idempotency for tracked IDs vs cache visibility.
4. Add a unit test that asserts no repeated cancel bursts within the throttle window on sustained staleness.

**Verify:**

1. `pytest tests/unit_tests/flux/strategies/makerv3/test_single_leg_quoter_strategy.py -q`
2. `pytest tests/unit_tests/examples/strategies/test_makerv3_single_leg_quoter.py -q`

### Task 2: Remove unsafe cancel-all behavior (strategy-owned cancellation only)

**Files:**

- Modify: `nautilus_trader/flux/strategies/makerv3/single_leg_quoter.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_single_leg_quoter_strategy.py`

**Steps:**

1. Change `_cancel_managed_quotes` so default behavior cancels only strategy-owned orders (as returned by `_managed_orders()`).
2. If a “cancel all instrument orders” escape hatch is required, gate it behind an explicit config flag which defaults to `False`.
3. Ensure per-order cancel exceptions are counted and emitted in one structured event (not spammy per order).

**Verify:**

1. `pytest tests/unit_tests/flux/strategies/makerv3/test_single_leg_quoter_strategy.py -q`

### Task 3: Fix stale-data cancel burst behavior

**Files:**

- Modify: `nautilus_trader/flux/strategies/makerv3/single_leg_quoter.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_single_leg_quoter_strategy.py`

**Steps:**

1. Add a dedupe/cooldown for stale-triggered cancel so sustained staleness does not repeat cancel loops every quote throttle tick.
2. Ensure quote throttle state (`_last_requote_ns` or equivalent) updates even when blocked due to staleness to prevent immediate re-entry.
3. Emit a state transition event only on transitions into/out of blocked states.

**Verify:**

1. `pytest tests/unit_tests/flux/strategies/makerv3/test_single_leg_quoter_strategy.py -q`

### Task 4: Wire runtime params manager and unify param schema

**Files:**

- Create: `nautilus_trader/flux/common/params.py`

**Steps:**

1. Introduce a `RuntimeParamRegistry` with canonical schema/defaults/constraints for `makerv3`.
2. Include min/max bounds for hot-path sensitive params (`n_orders*`, match tolerances, stale budgets, etc.).
3. Provide a “diff summary” helper for logging/alert payloads on param changes.

**Verify:**

1. `pytest tests/unit_tests/flux/strategies/makerv3 -q` (should remain green)

### Task 5: Update API defaults/schema to use the canonical registry

**Files:**

- Modify: `nautilus_trader/flux/api/app.py`
- Modify: `nautilus_trader/flux/params/manager.py` (if needed to accept registry metadata)

**Steps:**

1. Remove duplicated `DEFAULT_PARAMS_SCHEMA` / `DEFAULT_PARAMS_DEFAULTS` definitions for `makerv3`.
2. Source schema/defaults from `RuntimeParamRegistry` so strategy and API cannot drift.
3. Extend params pubsub payload to include schema metadata (`schema_version`, `param_set`, digest) for visibility.

**Verify:**

1. `pytest tests/unit_tests/flux/params -q` (if present)

### Task 6: Update strategy runtime params application and identity consistency

**Files:**

- Modify: `nautilus_trader/flux/strategies/makerv3/single_leg_quoter.py`
- Modify: `nautilus_trader/flux/common/keys.py` (if identity scoping requires)

**Steps:**

1. Replace local runtime param definitions with registry-driven initialization and update application.
2. Enforce one authoritative identity for:
   - Redis keyspace ownership
   - params key lookup
   - published payload `strategy_id`
3. Wire params manager in live runner (or provide a strategy-side factory) so runtime updates are effective.
4. Add tests for: unknown key rejection, bounded depth rejection, and stable identity behavior.

**Verify:**

1. `pytest tests/unit_tests/flux/strategies/makerv3 -q`

### Task 7: Hot-path performance tightening (no behavior change)

**Files:**

- Modify: `nautilus_trader/flux/strategies/makerv3/single_leg_quoter.py`

**Steps:**

1. Replace per-delta string conversions of BBO with Decimal tuple storage; stringify only at publish boundary.
2. Cache typed runtime params snapshot and reuse within `_refresh_quotes`.
3. Add short-TTL caching for inventory skew computation, invalidated on order/balance-affecting events.
4. Reduce `_managed_orders()` calls per quote cycle to one.

**Verify:**

1. `pytest tests/unit_tests/flux/strategies/makerv3 -q`

### Task 8: Introduce structured quote-cycle events and reason codes

**Files:**

- Create: `nautilus_trader/flux/strategies/makerv3/wire.py`
- Create: `nautilus_trader/flux/strategies/makerv3/constants.py`
- Modify: `nautilus_trader/flux/strategies/makerv3/single_leg_quoter.py`
- Modify: `docs/concepts/logging.md`

**Steps:**

1. Create a small event envelope schema (`run_id`, `quote_cycle_id`, `reason_code`, `ts_ms`, etc.).
2. Emit quote cycle events for: skipped (with reason), blocked (with transition), completed (with action counts).
3. Ensure alerts only fire on actionable conditions and are rate-limited (cooldown/transition-based).

**Verify:**

1. `pytest tests/unit_tests/flux/strategies/makerv3 -q`

### Task 9: Modularize strategy implementation (pricing/rebalance/inventory/managed orders)

**Files:**

- Create: `nautilus_trader/flux/strategies/makerv3/pricing.py`
- Create: `nautilus_trader/flux/strategies/makerv3/rebalancing.py`
- Create: `nautilus_trader/flux/strategies/makerv3/inventory.py`
- Create: `nautilus_trader/flux/strategies/makerv3/managed_orders.py`
- Modify: `nautilus_trader/flux/strategies/makerv3/single_leg_quoter.py`
- Modify: tests under `tests/unit_tests/flux/strategies/makerv3/` to import from modules, not underscored strategy helpers

**Steps:**

1. Move pure helpers first (pricing + rebalancing) with unit tests.
2. Move inventory logic with dedicated unit tests and caching semantics.
3. Move managed-order tracking/cancel helpers; keep cancellation safety invariant explicit in this module.
4. Shrink `single_leg_quoter.py` to an orchestrator that calls module functions.

**Verify:**

1. `pytest tests/unit_tests/flux/strategies/makerv3 -q`

### Task 10: Rename canonical strategy to `makerv3` and remove `single_leg_quoter` as the primary surface

**Files:**

- Create: `nautilus_trader/flux/strategies/makerv3/strategy.py`
- Modify: `nautilus_trader/flux/strategies/makerv3/__init__.py`
- Modify: `nautilus_trader/flux/strategies/__init__.py`
- Modify: `nautilus_trader/flux/strategies/makerv3/single_leg_quoter.py` (compat shim or delete)
- Modify: tests to import `MakerV3Strategy` from canonical module

**Steps:**

1. Introduce `MakerV3Strategy` and `MakerV3StrategyConfig` in `strategy.py`.
2. Update exports so canonical import is `nautilus_trader.flux.strategies.makerv3.MakerV3Strategy`.
3. Convert `single_leg_quoter.py` into a compatibility shim (or remove if all callsites updated).
4. Update topic constants to `flux.makerv3.*` and implement any transitional publish if needed.

**Verify:**

1. `pytest tests/unit_tests/flux/strategies/makerv3 -q`

### Task 11: Remove duplicated example strategy logic (thin wrapper only)

**Files:**

- Modify: `nautilus_trader/examples/strategies/makerv3_single_leg_quoter.py`
- Modify: `tests/unit_tests/examples/strategies/test_makerv3_single_leg_quoter.py`

**Steps:**

1. Replace the example strategy implementation with a thin wrapper that imports and configures the canonical `MakerV3Strategy`.
2. Remove any strategy logic duplication in `examples/` (examples should not be the canonical implementation).
3. Keep example-specific hardcoding only inside example runner/wrapper code, never in production modules.

**Verify:**

1. `pytest tests/unit_tests/examples/strategies/test_makerv3_single_leg_quoter.py -q`

---

## Open questions / decisions to make early

1. Is “cancel all orders for instrument” ever required operationally? If yes, under what explicit config and guardrails?
2. What is the authoritative identity field for strategy scoping across keys, topics, and payloads (single `strategy_id` vs dual external IDs)?
3. What is the maximum supported quote depth (cap) for runtime params in production, and what is the correct reject behavior on oversize updates?
4. Should topic migration publish to both old and new namespaces for a transition period, or should consumers be updated in lockstep?
