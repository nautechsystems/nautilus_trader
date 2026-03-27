# Shared Deque Quote Stack Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Replace MakerV3's current bounded-convergence stack behavior with a simpler shared deque-style planner, while completing any missing quote-action telemetry required to audit the new behavior in the same PR.

**Architecture:** Add a new pure shared `quote_stack` planner that emits only front/back mutations plus explicit hole repair, then integrate MakerV3 onto it while preserving existing risk and stale-market-data safety gates. Remove resting-order TTL from normal stack maintenance, keep operator-visible quoting knobs minimal, and ensure `quote_cycle` and `order_action` persist enough reason/level metadata to prove the deque contract in production.

**Tech Stack:** Python, Nautilus Trader live strategy runtime, Flux MakerV3, shared strategy utilities, SQLite persistence, pytest.

**Context Docs:**
- Design: `docs/plans/2026-03-26-shared-deque-quote-stack-design.md`
- PRD: `none`
- Relevant specs/runbooks: `systems/flux/docs/makerv3.md`, `docs/runbooks/tokenmm-makerv3-bounded-convergence-rollout.md`, `research/tokenmm/README.md`

**Decision Summary:**
- Introduce a new shared pure deque planner rather than further simplifying the current bounded-convergence planner in place.
- Allow temporary `N+1` only for the normal inward-move path (`place_front` then `cancel_back`).
- Remove resting-order TTL from normal quote maintenance; `max_age_ms` remains market-data freshness only.
- Keep production safety gates intact and fill missing telemetry in the same PR if the current live persistence path is incomplete.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | completed | controller | none | `systems/flux/flux/strategies/shared`, `systems/flux/flux/strategies/makerv3`, `nautilus_trader/persistence/orders`, `tests/unit_tests/flux/strategies/shared`, `tests/unit_tests/flux/strategies/makerv3`, `tests/unit_tests/persistence`, `systems/flux/docs`, `docs/runbooks`, `research/tokenmm`, `docs/plans` | `codex/shared-deque-quote-stack` | `.worktrees/shared-deque-quote-stack` | final closeout commit pending | Targeted verification bundle is green | All planned tasks are complete. Task 7 verification exposed one last outdated rebalancing test expectation for the old cancel taxonomy; that test is updated locally and the final closeout commit is the only remaining bookkeeping step |
| Task 1: Lock the deque planner contract in red tests | completed | main | none | `tests/unit_tests/flux/strategies/shared/test_quote_stack.py`, `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py` | `codex/shared-deque-quote-stack` | `.worktrees/shared-deque-quote-stack` | `60487a96da` | red_confirmed | Reviewed and committed. Shared tests fail per case on missing `quote_stack`; makerv3 targeted tests fail on missing `stack_action_mode` and current `cancel_stale_order` churn. Residual risk: sell-side mirror coverage deferred by scope. |
| Task 2: Implement the shared pure quote-stack planner | completed | main | Task 1: Lock the deque planner contract in red tests | `systems/flux/flux/strategies/shared/quote_stack.py`, `systems/flux/flux/strategies/shared/__init__.py`, `tests/unit_tests/flux/strategies/shared/test_quote_stack.py` | `codex/shared-deque-quote-stack` | `.worktrees/shared-deque-quote-stack` | `51cada578e` | shared_tests_pass | Reviewed and committed. Shared planner is pure, passes `13` shared tests, and fixes cancel-edge, overflow, dataclass normalization, and alias-signature issues. Residual risk: sell-side symmetry and root re-export coverage deferred. |
| Task 3: Integrate MakerV3 onto the shared deque planner | completed | main | Task 2: Implement the shared pure quote-stack planner | `systems/flux/flux/strategies/makerv3/quote_engine.py`, `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv3/rebalancing.py`, `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py`, `tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py` | `codex/shared-deque-quote-stack` | `.worktrees/shared-deque-quote-stack` | `1a57edcbbf` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py` -> `66 passed in 0.08s` | Tracker corrected after verification: shared deque integration already exists in branch ancestry at `1a57edcbbf`; no new Task 3 diff was required in this session |
| Task 4: Remove resting-order TTL from normal quote maintenance | completed | main | Task 3: Integrate MakerV3 onto the shared deque planner | `systems/flux/flux/common/params.py`, `systems/flux/flux/strategies/makerv3/quote_engine.py`, `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv3/runtime_params.py`, `deploy/tokenmm/strategies/plumeusdt_bybit_perp_makerv3.toml`, `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py` | `codex/shared-deque-quote-stack` | `.worktrees/shared-deque-quote-stack` | `7acd828f3f`, `cae579e9ba`, `ad47f8da39` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py -k 'does_not_consult_resting_order_age_for_rebalance'` -> `1 passed`; `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py -k 'max_age_ms_as_market_data_freshness_only'` -> `1 passed`; `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py` -> `61 passed`; `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv3/test_order_safety.py -k '_rebalance_side or rebalance_side'` -> `2 passed` | Review-complete: normal quote maintenance no longer consults order age, deploy/runtime schema surfaces are aligned, and the remaining `_rebalance_side(cancel_actions=None)` test gap was accepted as non-blocking |
| Task 5: Complete quote-action telemetry in quote-cycle and order_action | completed | main | Task 3: Integrate MakerV3 onto the shared deque planner | `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv3/quote_engine.py`, `systems/flux/flux/strategies/makerv3/rebalancing.py`, `systems/flux/flux/strategies/makerv3/constants.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`, `tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py` | `codex/shared-deque-quote-stack` | `.worktrees/shared-deque-quote-stack` | `799753e542`, `93c9e7100c` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -k 'deque_diagnostics'` -> `1 passed`; `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py -k 'place_front_then_cancel_back or cancel_front_then_place_back or runtime_strategy_id_and_context or structured_reason_taxonomy'` -> `6 passed`; `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py` -> `51 passed in 0.09s`; `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -p pytest_asyncio.plugin -q tests/unit_tests/persistence/test_order_action_persistence_actor.py tests/unit_tests/persistence/test_order_action_sqlite.py` -> `39 passed in 1.06s` | Completed by combining the already-landed code commit `799753e542` with test coverage commit `93c9e7100c`; persistence enrichment was already present in ancestry, and manual controller review confirmed the diff after reviewer lanes failed to return in this environment |
| Task 6: Update docs and operator audit surfaces | completed | main | Task 5: Complete quote-action telemetry in quote-cycle and order_action | `systems/flux/docs/makerv3.md`, `docs/runbooks/tokenmm-makerv3-bounded-convergence-rollout.md`, `research/tokenmm/telemetry_helpers.py`, `research/tokenmm/README.md`, `tests/unit_tests/docs`, `docs/plans/2026-03-26-shared-deque-quote-stack-design.md`, `docs/plans/2026-03-26-shared-deque-quote-stack.md` | `codex/shared-deque-quote-stack` | `.worktrees/shared-deque-quote-stack` | `59dff7c081` | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/python - <<'PY' ... import extract_quote_cycle_deque_diagnostics, extract_order_action_deque_audit ...` -> `telemetry_helpers_ok`; `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -p pytest_asyncio.plugin -q tests/unit_tests/docs/test_makerv3_markouts_docs.py tests/unit_tests/docs/test_makerv3_doc_links.py` -> `2 passed in 0.01s` | Completed. MakerV3 docs now describe front/back-only normal repricing, no resting-order TTL refresh, canonical deque reason codes, and operator audit fields; runbook includes SQLite checks; research helpers expose flattening helpers for deque audits |
| Task 7: Run targeted verification and capture rollout-ready evidence | completed | main | Task 6: Update docs and operator audit surfaces | `tests/unit_tests/flux/strategies/shared/test_quote_stack.py`, `tests/unit_tests/flux/strategies/makerv3`, `tests/unit_tests/persistence`, `tests/unit_tests/docs`, `docs/plans/2026-03-26-shared-deque-quote-stack.md` | `codex/shared-deque-quote-stack` | `.worktrees/shared-deque-quote-stack` | final closeout commit pending | `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/strategies/shared/test_quote_stack.py` -> `13 passed in 0.43s`; `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py` -> initial `5 failed, 107 passed` due stale rebalancing test expectations, then rerun after test alignment -> `112 passed in 0.19s`; `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -p pytest_asyncio.plugin -q tests/unit_tests/persistence/test_order_action_persistence_actor.py tests/unit_tests/persistence/test_order_action_sqlite.py` -> `39 passed in 1.09s`; `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 .venv/bin/pytest -p pytest_asyncio.plugin -q tests/unit_tests/docs/test_makerv3_markouts_docs.py tests/unit_tests/docs/test_makerv3_doc_links.py` -> `2 passed in 0.02s`; `git diff --check` -> clean | Verification complete. The only follow-up needed during this task was updating `test_rebalancing.py` to expect the canonical deque cancel reasons emitted by Task 5, after which the full targeted bundle passed cleanly |

---

### Task 1: Lock the deque planner contract in red tests

**Files:**
- Create: `tests/unit_tests/flux/strategies/shared/test_quote_stack.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py`

**Dependencies:** `none`

**Write Scope:** `tests/unit_tests/flux/strategies/shared/test_quote_stack.py`, `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q tests/unit_tests/flux/strategies/shared/test_quote_stack.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py -k 'deque or stale or hole or front or back'`

**Step 1: Write the failing shared-planner tests**

Add red tests covering:

- stable fair value and matching stack => `no_op`
- inward move => `place_front` then `cancel_back`
- outward move => `cancel_front` then `place_back`
- no middle cancel when the stack is otherwise valid
- short-stack hole repair places a missing level without canceling another order
- full-depth hole repair cancels the blocking order, then places the missing level on the next cycle
- temporary `N+1` is represented explicitly for inward moves

**Step 2: Run test to verify it fails**

Run: `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q tests/unit_tests/flux/strategies/shared/test_quote_stack.py`
Expected: FAIL because `quote_stack.py` does not exist yet.

**Step 3: Add Makerv3 regression tests**

Extend Makerv3 tests to fail on current undesired behavior:

- stable-target cycles should not emit `cancel_stale_order`
- normal rebalanced cycles should not report room-creation or middle-reprice reasons
- deque transitions should show front/back-only action modes

**Step 4: Run Makerv3 regression tests to verify they fail**

Run: `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py -k 'deque or stale or hole or front or back'`
Expected: FAIL against the current bounded-convergence behavior.

**Step 5: Commit**

```bash
git add tests/unit_tests/flux/strategies/shared/test_quote_stack.py \
  tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py \
  tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py
git commit -m "test: lock deque quote stack contract"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Implement the shared pure quote-stack planner

**Files:**
- Create: `systems/flux/flux/strategies/shared/quote_stack.py`
- Modify: `systems/flux/flux/strategies/shared/__init__.py`
- Modify: `tests/unit_tests/flux/strategies/shared/test_quote_stack.py`

**Dependencies:** `Task 1: Lock the deque planner contract in red tests`

**Write Scope:** `systems/flux/flux/strategies/shared/quote_stack.py`, `systems/flux/flux/strategies/shared/__init__.py`, `tests/unit_tests/flux/strategies/shared/test_quote_stack.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q tests/unit_tests/flux/strategies/shared/test_quote_stack.py`

**Step 1: Write the planner dataclasses and public API**

Implement a pure side-local planner surface with explicit types, for example:

- `ActiveStackLevel`
- `DesiredStackLevel`
- `StackAction`
- `StackPlan`
- `plan_side_deque_actions(...)`

The public API must not depend on strategy objects, cache access, or wall clock.

**Step 2: Implement matching and deque rules**

Implement minimal helpers for:

- best-to-worst ordering
- match tolerance checks
- detection of missing levels
- detection of front cancel violation
- depth overflow on the back
- short-stack hole repair without paired repricing cancel
- full-depth hole repair with explicit `cancel_free_slot_for_missing_level`
- inward move represented as `place_front` then `cancel_back`
- outward move represented as `cancel_front` then `place_back`

**Step 3: Run the shared planner tests**

Run: `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q tests/unit_tests/flux/strategies/shared/test_quote_stack.py`
Expected: PASS.

**Step 4: Commit**

```bash
git add systems/flux/flux/strategies/shared/quote_stack.py \
  systems/flux/flux/strategies/shared/__init__.py \
  tests/unit_tests/flux/strategies/shared/test_quote_stack.py
git commit -m "feat: add shared deque quote stack planner"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Integrate MakerV3 onto the shared deque planner

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/quote_engine.py`
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv3/rebalancing.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py`

**Dependencies:** `Task 2: Implement the shared pure quote-stack planner`

**Write Scope:** `systems/flux/flux/strategies/makerv3/quote_engine.py`, `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv3/rebalancing.py`, `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py`, `tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py`

**Step 1: Replace the planner call site in quote refresh**

Modify the MakerV3 quote-refresh path to:

- build desired levels as it does today
- sort active orders best to worst
- call the new shared deque planner per side
- execute returned actions in planner order

**Step 2: Preserve production guards around the new planner**

Keep current:

- stale-market-data blocks
- pending-cancel freeze
- cancel-reject cooldown
- post-only clamp and uniqueness
- risk and bot-on gates

Do not reintroduce the removed middle-reprice behaviors through surrounding control flow.

**Step 3: Retire or reduce old bounded-convergence behavior**

Refactor `rebalancing.py` so it no longer drives the live middle-reprice behavior. It may become:

- compatibility helpers only
- thin adapters around the shared planner
- or dead code removed if the test surface allows it

Do not leave stale/room planner logic on the live path.

**Step 4: Run the Makerv3 integration tests**

Run: `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py`
Expected: PASS.

**Step 5: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/quote_engine.py \
  systems/flux/flux/strategies/makerv3/strategy.py \
  systems/flux/flux/strategies/makerv3/rebalancing.py \
  tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py \
  tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py \
  tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py
git commit -m "feat: move makerv3 to shared deque stack planner"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Remove resting-order TTL from normal quote maintenance

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv3/runtime_params.py`
- Modify: `deploy/tokenmm/strategies/plumeusdt_bybit_perp_makerv3.toml`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py`

**Dependencies:** `Task 3: Integrate MakerV3 onto the shared deque planner`

**Write Scope:** `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv3/runtime_params.py`, `deploy/tokenmm/strategies/plumeusdt_bybit_perp_makerv3.toml`, `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`, `tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py`

**Step 1: Remove order TTL from the normal stack path**

Delete or bypass the code path that treats resting order age as a repricing signal.

`max_age_ms` must remain the market-data freshness input only.

**Step 2: Decide the config-compatibility behavior**

For this PR:

- keep existing config/runtime surfaces accepted
- do not add a new order TTL knob
- if the deployment config contains fields that no longer shape normal quoting, document that explicitly

**Step 3: Update red/green tests**

Add or update tests that prove:

- stable targets do not churn on age alone
- `max_age_ms` still blocks on stale market data
- no new operator knob is required

**Step 4: Run the focused tests**

Run: `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py`
Expected: PASS.

**Step 5: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/strategy.py \
  systems/flux/flux/strategies/makerv3/runtime_params.py \
  deploy/tokenmm/strategies/plumeusdt_bybit_perp_makerv3.toml \
  tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py \
  tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py
git commit -m "refactor: remove order ttl from normal quote maintenance"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 5: Complete quote-action telemetry in quote-cycle and order_action

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `nautilus_trader/persistence/orders/actor.py`
- Modify: `nautilus_trader/persistence/orders/schema.py`
- Modify: `nautilus_trader/persistence/orders/sqlite.py`
- Modify: `tests/unit_tests/persistence/test_order_action_persistence_actor.py`
- Modify: `tests/unit_tests/persistence/test_order_action_sqlite.py`
- Modify: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`

**Dependencies:** `Task 3: Integrate MakerV3 onto the shared deque planner`

**Write Scope:** `systems/flux/flux/strategies/makerv3/strategy.py`, `nautilus_trader/persistence/orders/actor.py`, `nautilus_trader/persistence/orders/schema.py`, `nautilus_trader/persistence/orders/sqlite.py`, `tests/unit_tests/persistence/test_order_action_persistence_actor.py`, `tests/unit_tests/persistence/test_order_action_sqlite.py`, `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q tests/unit_tests/persistence/test_order_action_persistence_actor.py tests/unit_tests/persistence/test_order_action_sqlite.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`

**Step 1: Add or update quote-cycle diagnostics**

Emit per-side deque diagnostics such as:

- `stack_action_mode`
- `front_changed`
- `back_changed`
- `depth_before`
- `depth_after`
- `missing_level_count`
- `interior_hole_count`

**Step 2: Ensure order-intent carries the new reason taxonomy**

Make sure the MakerV3 intent publisher emits canonical reason codes for the new stack path, for example:

- `cancel_front_violation`
- `cancel_back_excess`
- `cancel_free_slot_for_missing_level` for exceptional full-depth hole repair
- `place_front_improve`
- `place_back_backfill`
- `place_missing_hole_repair`

**Step 3: Fix persistence enrichment if live lifecycle rows are currently missing it**

Make the persistence actor reliably enrich `order_action` rows with:

- `reason_code`
- `level_index`
- `quote_cycle_id`
- any new stack-action context required for audits

This task is complete only when the tests prove the enrichment path works on the persisted rows, not just on the producer topic.

**Step 4: Run persistence and observability tests**

Run:

- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q tests/unit_tests/persistence/test_order_action_persistence_actor.py tests/unit_tests/persistence/test_order_action_sqlite.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`

Expected: PASS.

**Step 5: Commit**

```bash
git add systems/flux/flux/strategies/makerv3/strategy.py \
  nautilus_trader/persistence/orders/actor.py \
  nautilus_trader/persistence/orders/schema.py \
  nautilus_trader/persistence/orders/sqlite.py \
  tests/unit_tests/persistence/test_order_action_persistence_actor.py \
  tests/unit_tests/persistence/test_order_action_sqlite.py \
  tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py
git commit -m "feat: complete deque quote action telemetry"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 6: Update docs and operator audit surfaces

**Files:**
- Modify: `systems/flux/docs/makerv3.md`
- Modify: `docs/runbooks/tokenmm-makerv3-bounded-convergence-rollout.md`
- Modify: `research/tokenmm/telemetry_helpers.py`
- Modify: `research/tokenmm/README.md`
- Modify: `docs/plans/2026-03-26-shared-deque-quote-stack-design.md`
- Modify: `docs/plans/2026-03-26-shared-deque-quote-stack.md`

**Dependencies:** `Task 5: Complete quote-action telemetry in quote-cycle and order_action`

**Write Scope:** `systems/flux/docs/makerv3.md`, `docs/runbooks/tokenmm-makerv3-bounded-convergence-rollout.md`, `research/tokenmm/telemetry_helpers.py`, `research/tokenmm/README.md`, `docs/plans/2026-03-26-shared-deque-quote-stack-design.md`, `docs/plans/2026-03-26-shared-deque-quote-stack.md`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q tests/unit_tests/docs/test_makerv3_markouts_docs.py tests/unit_tests/docs/test_makerv3_doc_links.py`

**Step 1: Update the MakerV3 docs**

Document:

- front/back-only normal repricing
- no resting-order TTL refresh
- hole-repair semantics
- telemetry fields operators should query

**Step 2: Update rollout and research guidance**

Make the rollout runbook and telemetry helper docs point operators toward:

- `quote_cycle.stack_action_mode`
- persisted order-action reason codes and level indices
- explicit SQLite checks for front/back-only mutation

**Step 3: Run the docs tests**

Run: `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q tests/unit_tests/docs/test_makerv3_markouts_docs.py tests/unit_tests/docs/test_makerv3_doc_links.py`
Expected: PASS.

**Step 4: Commit**

```bash
git add systems/flux/docs/makerv3.md \
  docs/runbooks/tokenmm-makerv3-bounded-convergence-rollout.md \
  research/tokenmm/telemetry_helpers.py \
  research/tokenmm/README.md \
  docs/plans/2026-03-26-shared-deque-quote-stack-design.md \
  docs/plans/2026-03-26-shared-deque-quote-stack.md
git commit -m "docs: describe shared deque quote stack behavior"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 7: Run targeted verification and capture rollout-ready evidence

**Files:**
- Modify: `docs/plans/2026-03-26-shared-deque-quote-stack.md`

**Dependencies:** `Task 6: Update docs and operator audit surfaces`

**Write Scope:** `docs/plans/2026-03-26-shared-deque-quote-stack.md`

**Verification Commands:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q tests/unit_tests/flux/strategies/shared/test_quote_stack.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q tests/unit_tests/flux/strategies/makerv3/test_rebalancing.py tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/strategies/makerv3/test_order_intent_exports.py tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q tests/unit_tests/persistence/test_order_action_persistence_actor.py tests/unit_tests/persistence/test_order_action_sqlite.py`
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q tests/unit_tests/docs/test_makerv3_markouts_docs.py tests/unit_tests/docs/test_makerv3_doc_links.py`
- `git diff --check`

**Step 1: Run the full targeted verification bundle**

Run each verification command above and record pass/fail results in the tracker immediately.

**Step 2: Record operator-facing evidence in the plan notes**

Capture:

- the final verification command results
- any manual SQLite query examples needed for rollout
- the fact that telemetry now proves front/back-only mutation

**Step 3: Commit**

```bash
git add docs/plans/2026-03-26-shared-deque-quote-stack.md
git commit -m "docs: record deque quote stack verification"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.
