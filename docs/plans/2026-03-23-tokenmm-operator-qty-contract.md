# TokenMM Operator Quantity Contract Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Make every operator-facing TokenMM quantity surface use base-asset units by default so `qty=1000` always means `1000 PLUME`, while preserving venue-native contract quantities plus conversion provenance as explicit secondary/debug fields.

**Architecture:** Keep venue-native quantities inside adapters, order submission, and exchange reconciliation. Persist and publish explicit normalized fields (`*_base`, `*_venue`, `qty_conversion_status`, `qty_conversion_source`) for new telemetry and trade events, but do not flip the shared cross-strategy producer contract by redefining bare `qty` there. Instead, keep shared producer rows backward-compatible and apply the base-first semantic flip only inside TokenMM-facing API/socket/Fluxboard projections. Do not silently repurpose legacy raw SQLite or Redis trade rows; instead, add explicit normalized columns for newly persisted rows, document direct-DB caveats for pre-rollout rows, and require a TokenMM trade-stream cutover/reset during rollout because legacy bare Redis `qty` rows cannot be safely reinterpreted without producer-supplied normalized fields.

**Tech Stack:** Python 3.12, SQLite telemetry persistence, Redis stream payloads, Flask Flux API, Socket.IO delta streams, React/TypeScript Fluxboard, pytest, Vitest.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Task 1: Lock the regression matrix for operator-facing quantities | completed | main | none | `tests/unit_tests/persistence/test_execution_fill_sqlite.py`, `tests/unit_tests/persistence/test_order_action_sqlite.py`, `tests/unit_tests/flux/strategies/shared/test_trades.py`, `tests/unit_tests/flux/api/test_payloads.py`, `tests/unit_tests/flux/api/test_app.py`, `tests/unit_tests/flux/api/test_tokenmm_compat.py`, `tests/unit_tests/flux/api/test_socketio_tokenmm.py`, `fluxboard/api.flux.test.ts`, `fluxboard/Trades.test.tsx`, `fluxboard/Trades.mobile.test.tsx`, `fluxboard/__tests__/trades-integration.test.tsx`, `fluxboard/Trades.recovery.test.tsx` | `codex/tokenmm-operator-qty-contract-20260323` | `/home/ubuntu/nautilus_trader/.worktrees/tokenmm-operator-qty-contract-20260323` | `0e2b337cd8..ea6dea6d7e` | `python3 -m py_compile PASS; pytest Task 1 slice FAIL with 18 red tests (intended quantity-field/projection gaps plus a few pre-existing API/socket failures); fluxboard red suite FAIL with intended qty mismatch assertions and known pre-existing api.flux.test.ts failures; spec/quality reviews passed` | Task 1 regression-only test diff completed in commits `7b7b0dedf9`, `7d858dcfbf`, and `ea6dea6d7e`; compiled extensions mirrored into worktree so Python slice now runs on 2026-03-23 |
| Task 2: Persist base, venue, and provenance fields for new telemetry rows | completed | main | Task 1: Lock the regression matrix for operator-facing quantities | `nautilus_trader/persistence/_operator_quantity.py`, `nautilus_trader/persistence/fills/actor.py`, `nautilus_trader/persistence/fills/schema.py`, `nautilus_trader/persistence/fills/sqlite.py`, `nautilus_trader/persistence/orders/actor.py`, `nautilus_trader/persistence/orders/schema.py`, `nautilus_trader/persistence/orders/sqlite.py`, `nautilus_trader/persistence/shipper/postgres.py`, `tests/unit_tests/persistence/test_execution_fill_sqlite.py`, `tests/unit_tests/persistence/test_execution_fill_persistence_actor.py`, `tests/unit_tests/persistence/test_order_action_sqlite.py`, `tests/unit_tests/persistence/test_order_action_persistence_actor.py`, `tests/unit_tests/persistence/test_telemetry_shipper.py` | `codex/tokenmm-operator-qty-contract-20260323` | `/home/ubuntu/nautilus_trader/.worktrees/tokenmm-operator-qty-contract-20260323` | `ea6dea6d7e..a31d2fab00` | `pytest tests/unit_tests/persistence/test_execution_fill_sqlite.py tests/unit_tests/persistence/test_execution_fill_persistence_actor.py tests/unit_tests/persistence/test_order_action_sqlite.py tests/unit_tests/persistence/test_order_action_persistence_actor.py tests/unit_tests/persistence/test_telemetry_shipper.py -v PASS (84 passed in 2.54s)` | Spec review passed on 2026-03-23; controller quality review found no remaining correctness or migration issues in the Task 2 persistence diff |
| Task 3: Add explicit normalized fields to shared trade rows without flipping bare qty semantics | completed | main | Task 1: Lock the regression matrix for operator-facing quantities | `systems/flux/flux/strategies/shared/trades.py`, `tests/unit_tests/flux/strategies/shared/test_trades.py`, `systems/flux/docs/makerv3.md` | `codex/tokenmm-operator-qty-contract-20260323` | `/home/ubuntu/nautilus_trader/.worktrees/tokenmm-operator-qty-contract-20260323` | `a31d2fab00..f3a64dab3b` | `pytest tests/unit_tests/flux/strategies/shared/test_trades.py -q PASS (2 passed in 0.38s); python3 -m py_compile systems/flux/flux/strategies/shared/trades.py PASS` | Spec review passed and quality re-review passed on 2026-03-23; shared producer keeps venue-native `qty` while publishing explicit normalized fields |
| Task 4: Make TokenMM API and socket projections base-first | completed | main | Task 2: Persist base, venue, and provenance fields for new telemetry rows; Task 3: Add explicit normalized fields to shared trade rows without flipping bare qty semantics | `systems/flux/flux/api/_payloads_common.py`, `systems/flux/flux/api/app.py`, `systems/flux/flux/api/socketio.py`, `tests/unit_tests/flux/api/test_payloads.py`, `tests/unit_tests/flux/api/test_app.py`, `tests/unit_tests/flux/api/test_tokenmm_compat.py`, `tests/unit_tests/flux/api/test_socketio_tokenmm.py` | `codex/tokenmm-operator-qty-contract-20260323` | `/home/ubuntu/nautilus_trader/.worktrees/tokenmm-operator-qty-contract-20260323` | `f3a64dab3b..8421a53bc5` | `pytest tests/unit_tests/flux/api/test_payloads.py::test_build_trades_rows_prefers_explicit_base_qty_for_operator_contract tests/unit_tests/flux/api/test_payloads.py::test_build_trades_rows_keeps_generic_qty_venue_native_without_base_first_projection tests/unit_tests/flux/api/test_tokenmm_compat.py::test_trades_and_delta_project_base_qty_when_explicit_fields_are_present tests/unit_tests/flux/api/test_socketio_tokenmm.py::test_socket_emitter_trade_update_projects_base_qty_when_explicit_fields_are_present -q PASS (4 passed in 1.03s); pytest tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/flux/api/test_tokenmm_compat.py tests/unit_tests/flux/api/test_socketio_tokenmm.py -q FAIL only on the two known pre-existing socket delta tests` | Spec review passed; quality re-review passed on 2026-03-23 after scoping the base-first flip to TokenMM projections in commit `8421a53bc5` |
| Task 5: Update Fluxboard, research, and exporter surfaces to use canonical base quantity | completed | main | Task 4: Make TokenMM API and socket projections base-first | `fluxboard/types.ts`, `fluxboard/api.ts`, `fluxboard/stores.ts`, `fluxboard/Trades.tsx`, `fluxboard/components/trades/columns.tsx`, `fluxboard/components/trades/TradesTable.tsx`, `fluxboard/components/trades/rollups.ts`, `fluxboard/api.flux.test.ts`, `fluxboard/Trades.test.tsx`, `fluxboard/Trades.mobile.test.tsx`, `fluxboard/__tests__/trades-integration.test.tsx`, `fluxboard/Trades.recovery.test.tsx`, `fluxboard/components/trades/rollups.test.ts`, `research/tokenmm/telemetry_helpers.py`, `research/tokenmm/README.md`, `ops/scripts/exporters/tokenmm_markouts_exporter.py`, `tests/unit_tests/research/test_telemetry_helpers.py`, `tests/unit_tests/ops/test_tokenmm_markouts_exporter.py` | `codex/tokenmm-operator-qty-contract-20260323` | `/home/ubuntu/nautilus_trader/.worktrees/tokenmm-operator-qty-contract-20260323` | `8421a53bc5..603d9ea168` | `python3 -m py_compile research/tokenmm/telemetry_helpers.py ops/scripts/exporters/tokenmm_markouts_exporter.py PASS; pytest tests/unit_tests/research/test_telemetry_helpers.py tests/unit_tests/ops/test_tokenmm_markouts_exporter.py -q PASS (24 passed in 0.49s); cd fluxboard && VITEST_FULL=1 pnpm test:run api.flux.test.ts Trades.test.tsx Trades.mobile.test.tsx __tests__/trades-integration.test.tsx Trades.recovery.test.tsx components/trades/rollups.test.ts FAIL only on 3 known pre-existing api.flux.test.ts patchStrategy/updateParams tests; all Task 5 trade-surface files in that run passed; spec re-review passed; quality review passed` | Task 5 completed in commits `16caf71ace`, `16017ad47c`, and `603d9ea168`; Fluxboard now keeps non-TokenMM trade qty venue-native while projecting base-first qty only on TokenMM routes, and research/exporter helpers prefer normalized base quantity with legacy fallback |
| Task 6: Update TokenMM contracts, rollout notes, and historical-row caveats | completed | main | Task 5: Update Fluxboard, research, and exporter surfaces to use canonical base quantity | `fluxboard/docs/tokenmm_contract.md`, `fluxboard/docs/tokenmm_socket_contract.md`, `docs/plans/2026-03-23-tokenmm-operator-qty-contract.md`, `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py` | `codex/tokenmm-operator-qty-contract-20260323` | `/home/ubuntu/nautilus_trader/.worktrees/tokenmm-operator-qty-contract-20260323` | `16017ad47c..9122dbceca` | `pytest tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py -q PASS (51 passed in 0.09s)` | TokenMM REST/socket contract docs now define base-first operator `qty`, preserve shared-producer venue-native `qty` as an explicit note, and document the historical-row caveat plus required trade-stream cutover/reset |
| Task 7: Run end-to-end verification for OKX contract-multiplier scenarios | completed | main | Task 6: Update TokenMM contracts, rollout notes, and historical-row caveats | `tests/unit_tests/persistence`, `tests/unit_tests/flux`, `fluxboard`, `research/tokenmm`, `ops/scripts/exporters` | `codex/tokenmm-operator-qty-contract-20260323` | `/home/ubuntu/nautilus_trader/.worktrees/tokenmm-operator-qty-contract-20260323` | `9122dbceca..603d9ea168` | `pytest tests/unit_tests/persistence/test_execution_fill_sqlite.py tests/unit_tests/persistence/test_execution_fill_persistence_actor.py tests/unit_tests/persistence/test_order_action_sqlite.py tests/unit_tests/persistence/test_order_action_persistence_actor.py tests/unit_tests/persistence/test_telemetry_shipper.py tests/unit_tests/flux/strategies/shared/test_trades.py tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/flux/api/test_app.py tests/unit_tests/flux/api/test_tokenmm_compat.py tests/unit_tests/flux/api/test_socketio_tokenmm.py tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py tests/unit_tests/research/test_telemetry_helpers.py tests/unit_tests/ops/test_tokenmm_markouts_exporter.py -q FAIL only on 3 unrelated pre-existing tests (`test_socket_emitter_tokenmm_market_update_reports_changed_allowlisted_signals`, `test_socket_emitter_tokenmm_market_update_reports_alert_changes_from_secondary_strategy`, `test_signals_profile_tokenmm_overlays_portfolio_inventory_metadata_onto_rows`); rerun totals: 348 passed, 3 failed in 11.81s on 2026-03-23. cd fluxboard && VITEST_FULL=1 pnpm test:run api.flux.test.ts Trades.test.tsx Trades.mobile.test.tsx __tests__/trades-integration.test.tsx Trades.recovery.test.tsx components/trades/rollups.test.ts FAIL only on 3 known pre-existing api.flux.test.ts params-write tests (`api.patchStrategyParams > treats HTTP 200 responses with data.errors as save failure`, `api.patchStrategyParams > appends profile to params writes on equities routes`, `api.updateParams > appends profile to bulk params writes on equities routes`); rerun totals: 78 passed, 3 failed in 4.99s on 2026-03-23` | Full targeted verification was rerun after the final Task 5 fix; all quantity-contract surfaces remained green, and the only remaining failures are pre-existing tests outside the Task 5/6 write scope |

---

## Investigation Summary

- The current live OKX `PLUME-USDT-SWAP` contract uses `ctVal=10`, so `1000` PLUME base quantity becomes `100` venue contracts. Execution is correct; operator-facing quantity semantics are not.
- `deploy/tokenmm/strategies/plumeusdt_okx_perp_makerv3.toml` already declares `qty = "1000"` and `qty_unit = "base"`, so operator intent is unambiguous.
- `systems/flux/flux/strategies/makerv3/runtime_params.py` correctly converts base quantity to venue quantity before submit using `flux.common.quantity_units`.
- `nautilus_trader/persistence/fills/sqlite.py`, `nautilus_trader/persistence/orders/actor.py`, and `systems/flux/flux/strategies/shared/trades.py` currently persist or publish raw venue-native quantities without a normalized base companion field.
- The REST/socket API path that serves trades does not have an instrument-metadata lookup today; it only sees Redis trade row dicts, so legacy bare `qty` rows in Redis cannot be safely reinterpreted after the fact.
- Bybit and Bitget looked “correct” only because their native quantity currently matches PLUME base units; OKX exposed the hidden contract bug because its contract multiplier differs from `1`.
- The March 7, 2026 TokenMM quantity-contract docs explicitly codified venue/native trade qty semantics, so fixing this is a deliberate contract reversal for operator-facing quantity fields and requires docs, tests, and UI changes together.

## Non-Goals

- Do not change exchange adapter parsing or venue-native order submission logic.
- Do not change strategy config semantics; `qty` already means base units for operator input.
- Do not mutate historical raw SQLite `last_qty` / `order_qty` values in place without an explicit, proven-safe backfill path.
- Do not redefine bare `qty` on the shared `flux.makerv3.trade` producer topic for non-TokenMM consumers.
- Do not special-case OKX or PLUME in the implementation.

### Task 1: Lock the regression matrix for operator-facing quantities

**Files:**
- Modify: `tests/unit_tests/persistence/test_execution_fill_sqlite.py`
- Modify: `tests/unit_tests/persistence/test_order_action_sqlite.py`
- Modify: `tests/unit_tests/persistence/test_execution_fill_persistence_actor.py`
- Modify: `tests/unit_tests/persistence/test_order_action_persistence_actor.py`
- Modify: `tests/unit_tests/flux/strategies/shared/test_trades.py`
- Modify: `tests/unit_tests/flux/api/test_payloads.py`
- Modify: `tests/unit_tests/flux/api/test_app.py`
- Modify: `tests/unit_tests/flux/api/test_tokenmm_compat.py`
- Modify: `tests/unit_tests/flux/api/test_socketio_tokenmm.py`
- Modify: `fluxboard/api.flux.test.ts`
- Modify: `fluxboard/Trades.test.tsx`
- Modify: `fluxboard/Trades.mobile.test.tsx`
- Modify: `fluxboard/__tests__/trades-integration.test.tsx`
- Modify: `fluxboard/Trades.recovery.test.tsx`

**Dependencies:** `none`

**Write Scope:** `tests/unit_tests/persistence`, `tests/unit_tests/flux/strategies/shared/test_trades.py`, `tests/unit_tests/flux/api`, `fluxboard/api.flux.test.ts`, `fluxboard/Trades.test.tsx`, `fluxboard/Trades.mobile.test.tsx`, `fluxboard/__tests__/trades-integration.test.tsx`, `fluxboard/Trades.recovery.test.tsx`

**Verification Commands:**
- `pytest tests/unit_tests/persistence/test_execution_fill_sqlite.py tests/unit_tests/persistence/test_execution_fill_persistence_actor.py tests/unit_tests/persistence/test_order_action_sqlite.py tests/unit_tests/persistence/test_order_action_persistence_actor.py tests/unit_tests/flux/strategies/shared/test_trades.py tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/flux/api/test_app.py tests/unit_tests/flux/api/test_tokenmm_compat.py tests/unit_tests/flux/api/test_socketio_tokenmm.py -v`
- `cd fluxboard && pnpm test:run api.flux.test.ts Trades.test.tsx Trades.mobile.test.tsx __tests__/trades-integration.test.tsx Trades.recovery.test.tsx`

**Step 1: Add failing persistence expectations for explicit base and venue columns**

Add red tests that assert:
- an OKX-style fill persists `last_qty_base="1000"` and `last_qty_venue="100"` for a `multiplier=10` instrument
- an OKX-style order init persists `order_qty_base="1000"` and `order_qty_venue="100"`
- both persistence paths also carry `qty_conversion_status` and `qty_conversion_source`
- identity-multiplier venues still persist matching base and venue values

**Step 2: Add failing shared trade payload expectations**

Add a red test in `tests/unit_tests/flux/strategies/shared/test_trades.py` asserting that `build_trade_payload(...)` returns:
- `qty="100"`
- `qty_base="1000"`
- `qty_venue="100"`
- `qty_conversion_status="exact_multiplier"`
- `qty_conversion_source` naming the multiplier path

for an OKX-style contract-multiplier instrument.

**Step 3: Add failing API and socket contract expectations**

Add red tests that assert `/api/v1/trades` rows and `trade_update` delta payloads expose base-first `qty` while preserving an explicit venue field.

Minimum assertions:
- `qty == "1000"`
- `qty_base == "1000"`
- `qty_venue == "100"`
- `qty_conversion_status == "exact_multiplier"`
- pagination and row IDs remain stable

**Step 4: Add failing Fluxboard expectations**

Add red tests that assert the trades blotter:
- renders `1000` as the primary quantity for the OKX trade row
- computes notional/rollups from the canonical base `qty`
- preserves `qty_venue` through store, replay, and live socket update paths

**Step 5: Run the focused regression matrix**

Run:

```bash
pytest \
  tests/unit_tests/persistence/test_execution_fill_sqlite.py \
  tests/unit_tests/persistence/test_execution_fill_persistence_actor.py \
  tests/unit_tests/persistence/test_order_action_sqlite.py \
  tests/unit_tests/persistence/test_order_action_persistence_actor.py \
  tests/unit_tests/flux/strategies/shared/test_trades.py \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/flux/api/test_tokenmm_compat.py \
  tests/unit_tests/flux/api/test_socketio_tokenmm.py -v
cd fluxboard && pnpm test:run api.flux.test.ts Trades.test.tsx Trades.mobile.test.tsx __tests__/trades-integration.test.tsx Trades.recovery.test.tsx
```

Expected before implementation:
- the new persistence/API/UI assertions fail specifically on missing normalized base fields or on `qty` still reflecting venue-native contracts

**Step 6: Commit**

```bash
git add \
  tests/unit_tests/persistence/test_execution_fill_sqlite.py \
  tests/unit_tests/persistence/test_execution_fill_persistence_actor.py \
  tests/unit_tests/persistence/test_order_action_sqlite.py \
  tests/unit_tests/persistence/test_order_action_persistence_actor.py \
  tests/unit_tests/flux/strategies/shared/test_trades.py \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/flux/api/test_tokenmm_compat.py \
  tests/unit_tests/flux/api/test_socketio_tokenmm.py \
  fluxboard/api.flux.test.ts \
  fluxboard/Trades.test.tsx \
  fluxboard/Trades.mobile.test.tsx \
  fluxboard/__tests__/trades-integration.test.tsx \
  fluxboard/Trades.recovery.test.tsx
git commit -m "test(tokenmm): lock operator quantity contract regressions"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Persist base, venue, and provenance fields for new telemetry rows

**Files:**
- Create: `nautilus_trader/persistence/_operator_quantity.py`
- Modify: `nautilus_trader/persistence/fills/actor.py`
- Modify: `nautilus_trader/persistence/fills/schema.py`
- Modify: `nautilus_trader/persistence/fills/sqlite.py`
- Modify: `nautilus_trader/persistence/orders/actor.py`
- Modify: `nautilus_trader/persistence/orders/schema.py`
- Modify: `nautilus_trader/persistence/orders/sqlite.py`
- Modify: `nautilus_trader/persistence/shipper/postgres.py`
- Modify: `tests/unit_tests/persistence/test_execution_fill_sqlite.py`
- Modify: `tests/unit_tests/persistence/test_execution_fill_persistence_actor.py`
- Modify: `tests/unit_tests/persistence/test_order_action_sqlite.py`
- Modify: `tests/unit_tests/persistence/test_order_action_persistence_actor.py`
- Modify: `tests/unit_tests/persistence/test_telemetry_shipper.py`

**Dependencies:** `Task 1: Lock the regression matrix for operator-facing quantities`

**Write Scope:** `nautilus_trader/persistence/_operator_quantity.py`, `nautilus_trader/persistence/fills`, `nautilus_trader/persistence/orders`, `nautilus_trader/persistence/shipper/postgres.py`, `tests/unit_tests/persistence`

**Verification Commands:**
- `pytest tests/unit_tests/persistence/test_execution_fill_sqlite.py tests/unit_tests/persistence/test_execution_fill_persistence_actor.py tests/unit_tests/persistence/test_order_action_sqlite.py tests/unit_tests/persistence/test_order_action_persistence_actor.py tests/unit_tests/persistence/test_telemetry_shipper.py -v`

**Step 1: Add a shared normalization helper at the persistence layer**

Implement a helper that accepts:
- an instrument
- a venue/native quantity
- optional context for missing metadata

and returns a normalized payload:
- `qty_venue`
- `qty_base`
- `qty_conversion_status`
- `qty_conversion_source`

Use existing multiplier semantics already encoded in instrument metadata; do not duplicate OKX-specific logic here.

**Step 2: Snapshot normalized quantities before async persistence**

Update the fill and order persistence actors to resolve the instrument from cache on the hot path, compute both venue and base quantities, and enqueue those normalized values with the payload so the DB writer thread does not need live cache access.

If the instrument cannot be resolved:
- persist the raw quantity as `*_venue`
- leave `*_base` null
- set conversion provenance to an explicit missing-metadata status/source
- cover both cache-hit and cache-miss cases in actor tests

**Step 3: Extend SQLite and Postgres schemas with explicit quantity columns**

Add:
- `execution_fill.last_qty_base`
- `execution_fill.last_qty_venue`
- `execution_fill.qty_conversion_status`
- `execution_fill.qty_conversion_source`
- `order_action.order_qty_base`
- `order_action.order_qty_venue`
- `order_action.qty_conversion_status`
- `order_action.qty_conversion_source`

Keep existing raw `last_qty` / `order_qty` columns for backward compatibility during migration. New rows should continue writing the legacy raw column and also populate the new explicit columns.

**Step 4: Update row builders and schema migration coverage**

Update `ExecutionFillRow`, `OrderActionRow`, insert SQL, `ensure_schema`, and legacy-table migration tests so fresh DBs and upgraded DBs both expose the new columns without losing old data.

For Postgres, do not rely on `CREATE TABLE IF NOT EXISTS` alone. Extend `TelemetryPostgresSink.ensure_schema()` with explicit `ALTER TABLE ... ADD COLUMN IF NOT EXISTS ...` migration coverage before new inserts are attempted, and add a transport-level shipper test that exercises the new fields instead of only checking DDL strings.

**Step 5: Run the persistence slice**

Run:

```bash
pytest \
  tests/unit_tests/persistence/test_execution_fill_sqlite.py \
  tests/unit_tests/persistence/test_execution_fill_persistence_actor.py \
  tests/unit_tests/persistence/test_order_action_sqlite.py \
  tests/unit_tests/persistence/test_order_action_persistence_actor.py \
  tests/unit_tests/persistence/test_telemetry_shipper.py -v
```

Expected after implementation:
- PASS
- new schema columns exist in SQLite and Postgres definitions
- actor tests prove normalized base and venue quantities plus provenance are captured for multiplier, identity, and cache-miss instruments

**Step 6: Commit**

```bash
git add \
  nautilus_trader/persistence/_operator_quantity.py \
  nautilus_trader/persistence/fills/actor.py \
  nautilus_trader/persistence/fills/schema.py \
  nautilus_trader/persistence/fills/sqlite.py \
  nautilus_trader/persistence/orders/actor.py \
  nautilus_trader/persistence/orders/schema.py \
  nautilus_trader/persistence/orders/sqlite.py \
  nautilus_trader/persistence/shipper/postgres.py \
  tests/unit_tests/persistence/test_execution_fill_sqlite.py \
  tests/unit_tests/persistence/test_execution_fill_persistence_actor.py \
  tests/unit_tests/persistence/test_order_action_sqlite.py \
  tests/unit_tests/persistence/test_order_action_persistence_actor.py \
  tests/unit_tests/persistence/test_telemetry_shipper.py
git commit -m "feat(tokenmm): persist normalized operator quantity fields"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Add explicit normalized fields to shared trade rows without flipping bare qty semantics

**Files:**
- Modify: `systems/flux/flux/strategies/shared/trades.py`
- Modify: `tests/unit_tests/flux/strategies/shared/test_trades.py`
- Modify: `systems/flux/docs/makerv3.md`

**Dependencies:** `Task 1: Lock the regression matrix for operator-facing quantities`

**Write Scope:** `systems/flux/flux/strategies/shared/trades.py`, `tests/unit_tests/flux/strategies/shared/test_trades.py`, `systems/flux/docs/makerv3.md`

**Verification Commands:**
- `pytest tests/unit_tests/flux/strategies/shared/test_trades.py -v`

**Step 1: Keep shared producer bare qty backward-compatible**

Update `build_trade_payload(...)` so the shared producer continues publishing:
- bare `qty` as the existing raw venue-native execution quantity
- `qty_base` as the normalized operator quantity
- `qty_venue` as the explicit duplicate of raw venue size
- `qty_conversion_status`
- `qty_conversion_source`

Preserve row IDs, timestamps, side fields, commission fields, and non-TokenMM downstream semantics.

**Step 2: Document the shared-topic contract change narrowly**

Update the MakerV3 docs to clarify that shared producer rows now carry explicit normalized quantity fields, but bare `qty` remains venue-native on that topic.

**Step 3: Run the shared-producer slice**

Run:

```bash
pytest tests/unit_tests/flux/strategies/shared/test_trades.py -v
```

Expected after implementation:
- PASS
- shared producer rows carry explicit normalized fields and provenance without breaking existing non-TokenMM bare-qty semantics

**Step 4: Commit**

```bash
git add \
  systems/flux/flux/strategies/shared/trades.py \
  tests/unit_tests/flux/strategies/shared/test_trades.py \
  systems/flux/docs/makerv3.md
git commit -m "feat(tokenmm): add normalized quantity fields to shared trade rows"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Make TokenMM API and socket projections base-first

**Files:**
- Modify: `systems/flux/flux/api/_payloads_common.py`
- Modify: `systems/flux/flux/api/app.py`
- Modify: `systems/flux/flux/api/socketio.py`
- Modify: `tests/unit_tests/flux/api/test_payloads.py`
- Modify: `tests/unit_tests/flux/api/test_app.py`
- Modify: `tests/unit_tests/flux/api/test_tokenmm_compat.py`
- Modify: `tests/unit_tests/flux/api/test_socketio_tokenmm.py`

**Dependencies:** `Task 2: Persist base, venue, and provenance fields for new telemetry rows`; `Task 3: Add explicit normalized fields to shared trade rows without flipping bare qty semantics`

**Write Scope:** `systems/flux/flux/api/_payloads_common.py`, `systems/flux/flux/api/app.py`, `systems/flux/flux/api/socketio.py`, `tests/unit_tests/flux/api`

**Verification Commands:**
- `pytest tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/flux/api/test_app.py tests/unit_tests/flux/api/test_tokenmm_compat.py tests/unit_tests/flux/api/test_socketio_tokenmm.py -v`

**Step 1: Project TokenMM trades as base-first only when explicit normalized fields exist**

Update the TokenMM-facing API/socket normalization path so:
- canonical public `qty` is projected from `qty_base`
- `qty_base`, `qty_venue`, `qty_conversion_status`, and `qty_conversion_source` remain explicit
- generic or non-TokenMM trade paths do not silently change semantics

**Step 2: Make the rollout boundary explicit instead of guessing legacy conversions**

Do not attempt to derive base quantity inside the API from legacy bare `qty` or `qty_venue` rows, because the API stack has no instrument metadata resolver for Redis trade rows. Instead:
- require shared producers to emit explicit normalized fields before the projection flips `qty`
- document and test that TokenMM rollout requires a trade-stream cutover/reset so mixed old rows do not remain in the live blotter stream

**Step 3: Add route and socket coverage for the new projection contract**

Cover:
- `/api/v1/trades`
- `/api/v1/trades/delta`
- tokenmm profile filtering in `test_app.py`
- `trade_update` socket payloads

Run:

```bash
pytest \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/flux/api/test_tokenmm_compat.py \
  tests/unit_tests/flux/api/test_socketio_tokenmm.py -v
```

Expected after implementation:
- PASS
- TokenMM REST and socket payloads expose base-first `qty`
- projection never guesses at legacy Redis rows without explicit normalized fields

**Step 4: Commit**

```bash
git add \
  systems/flux/flux/api/_payloads_common.py \
  systems/flux/flux/api/app.py \
  systems/flux/flux/api/socketio.py \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/flux/api/test_tokenmm_compat.py \
  tests/unit_tests/flux/api/test_socketio_tokenmm.py
git commit -m "feat(tokenmm): project trade quantities as base-first in api"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 5: Update Fluxboard, research, and exporter surfaces to use canonical base quantity

**Files:**
- Modify: `fluxboard/types.ts`
- Modify: `fluxboard/api.ts`
- Modify: `fluxboard/stores.ts`
- Modify: `fluxboard/Trades.tsx`
- Modify: `fluxboard/components/trades/columns.tsx`
- Modify: `fluxboard/components/trades/TradesTable.tsx`
- Modify: `fluxboard/components/trades/rollups.ts`
- Modify: `fluxboard/api.flux.test.ts`
- Modify: `fluxboard/Trades.test.tsx`
- Modify: `fluxboard/Trades.mobile.test.tsx`
- Modify: `fluxboard/__tests__/trades-integration.test.tsx`
- Modify: `fluxboard/Trades.recovery.test.tsx`
- Modify: `fluxboard/components/trades/rollups.test.ts`
- Modify: `research/tokenmm/telemetry_helpers.py`
- Modify: `research/tokenmm/README.md`
- Modify: `ops/scripts/exporters/tokenmm_markouts_exporter.py`
- Modify: `tests/unit_tests/research/test_telemetry_helpers.py`
- Modify: `tests/unit_tests/ops/test_tokenmm_markouts_exporter.py`

**Dependencies:** `Task 4: Make TokenMM API and socket projections base-first`

**Write Scope:** `fluxboard/types.ts`, `fluxboard/api.ts`, `fluxboard/stores.ts`, `fluxboard/Trades.tsx`, `fluxboard/components/trades`, `fluxboard/api.flux.test.ts`, `fluxboard/Trades.test.tsx`, `fluxboard/Trades.mobile.test.tsx`, `fluxboard/__tests__/trades-integration.test.tsx`, `fluxboard/Trades.recovery.test.tsx`, `research/tokenmm/telemetry_helpers.py`, `research/tokenmm/README.md`, `ops/scripts/exporters/tokenmm_markouts_exporter.py`, `tests/unit_tests/research/test_telemetry_helpers.py`, `tests/unit_tests/ops/test_tokenmm_markouts_exporter.py`

**Verification Commands:**
- `cd fluxboard && pnpm test:run api.flux.test.ts Trades.test.tsx Trades.mobile.test.tsx __tests__/trades-integration.test.tsx Trades.recovery.test.tsx components/trades/rollups.test.ts`
- `pytest tests/unit_tests/research/test_telemetry_helpers.py tests/unit_tests/ops/test_tokenmm_markouts_exporter.py -v`

**Step 1: Preserve explicit quantity fields through the Fluxboard client/store path**

Update the Fluxboard client and stores so:
- canonical displayed `qty` is base-first
- `qty_venue` and conversion provenance are preserved through snapshot, delta, recovery, and replay paths
- the qty cell renders base quantity while keeping venue size available as secondary/debug data

**Step 2: Update rollups and live-recovery regression coverage**

Ensure tables, live updates, reconnect recovery, and rollups all use the canonical base `qty`, not raw contract counts.

**Step 3: Update research/exporter helpers that still read raw persistence qty columns**

Update research and export helpers to prefer `last_qty_base` / `order_qty_base` when available and fall back explicitly to legacy raw fields only when normalized columns are absent.

**Step 4: Re-run the client/research slice**

Run:

```bash
cd fluxboard && pnpm test:run \
  api.flux.test.ts \
  Trades.test.tsx \
  Trades.mobile.test.tsx \
  __tests__/trades-integration.test.tsx \
  Trades.recovery.test.tsx \
  components/trades/rollups.test.ts
pytest \
  tests/unit_tests/research/test_telemetry_helpers.py \
  tests/unit_tests/ops/test_tokenmm_markouts_exporter.py -v
```

Expected:
- PASS

**Step 5: Commit**

```bash
git add \
  fluxboard/types.ts \
  fluxboard/api.ts \
  fluxboard/stores.ts \
  fluxboard/Trades.tsx \
  fluxboard/components/trades/columns.tsx \
  fluxboard/components/trades/TradesTable.tsx \
  fluxboard/components/trades/rollups.ts \
  fluxboard/api.flux.test.ts \
  fluxboard/Trades.test.tsx \
  fluxboard/Trades.mobile.test.tsx \
  fluxboard/__tests__/trades-integration.test.tsx \
  fluxboard/Trades.recovery.test.tsx \
  fluxboard/components/trades/rollups.test.ts \
  research/tokenmm/telemetry_helpers.py \
  research/tokenmm/README.md \
  ops/scripts/exporters/tokenmm_markouts_exporter.py \
  tests/unit_tests/research/test_telemetry_helpers.py \
  tests/unit_tests/ops/test_tokenmm_markouts_exporter.py
git commit -m "feat(tokenmm): use base quantities across operator surfaces"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 6: Update TokenMM contracts, rollout notes, and historical-row caveats

**Files:**
- Modify: `fluxboard/docs/tokenmm_contract.md`
- Modify: `fluxboard/docs/tokenmm_socket_contract.md`
- Modify: `docs/plans/2026-03-23-tokenmm-operator-qty-contract.md`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`

**Dependencies:** `Task 5: Update Fluxboard, research, and exporter surfaces to use canonical base quantity`

**Write Scope:** `fluxboard/docs/tokenmm_contract.md`, `fluxboard/docs/tokenmm_socket_contract.md`, `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`

**Verification Commands:**
- `pytest tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py -v`

**Step 1: Rewrite the documented quantity contract**

Update docs so they state clearly:
- operator-facing `qty` is base quantity on TokenMM REST/socket/UI surfaces
- shared producer bare `qty` remains venue-native, with explicit `qty_base` and `qty_venue`
- conversion provenance fields are part of the public TokenMM contract

**Step 2: Document the migration boundary and rollout requirement**

Call out that:
- newly persisted rows carry explicit `*_base`, `*_venue`, and conversion provenance
- older raw SQLite rows may not have normalized columns
- legacy Redis trade rows cannot be safely reinterpreted by the API
- rollout therefore requires a TokenMM trade-stream cutover/reset before enabling the base-first projection in production

**Step 3: Update stack-contract coverage**

Extend the TokenMM stack contract test so it asserts the documented quantity contract and prevents future regressions back to venue-native bare `qty` on TokenMM-facing surfaces.

**Step 4: Re-run the contract doc/test slice**

Run: `pytest tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py -v`

Expected:
- PASS

**Step 5: Commit**

```bash
git add \
  fluxboard/docs/tokenmm_contract.md \
  fluxboard/docs/tokenmm_socket_contract.md \
  tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py
git commit -m "docs(tokenmm): define operator quantity contract"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 7: Run end-to-end verification for OKX contract-multiplier scenarios

**Files:**
- Reference: `tests/unit_tests/persistence`
- Reference: `tests/unit_tests/flux`
- Reference: `fluxboard`
- Reference: `research/tokenmm`
- Reference: `ops/scripts/exporters`
- Reference: `tests/unit_tests/research`
- Reference: `tests/unit_tests/ops`

**Dependencies:** `Task 6: Update TokenMM contracts, rollout notes, and historical-row caveats`

**Write Scope:** `none`

**Verification Commands:**
- `pytest tests/unit_tests/persistence/test_execution_fill_sqlite.py tests/unit_tests/persistence/test_execution_fill_persistence_actor.py tests/unit_tests/persistence/test_order_action_sqlite.py tests/unit_tests/persistence/test_order_action_persistence_actor.py tests/unit_tests/persistence/test_telemetry_shipper.py tests/unit_tests/flux/strategies/shared/test_trades.py tests/unit_tests/flux/api/test_payloads.py tests/unit_tests/flux/api/test_app.py tests/unit_tests/flux/api/test_tokenmm_compat.py tests/unit_tests/flux/api/test_socketio_tokenmm.py tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py -v`
- `cd fluxboard && pnpm test:run api.flux.test.ts Trades.test.tsx Trades.mobile.test.tsx __tests__/trades-integration.test.tsx Trades.recovery.test.tsx components/trades/rollups.test.ts`
- `pytest tests/unit_tests/research/test_telemetry_helpers.py tests/unit_tests/ops/test_tokenmm_markouts_exporter.py -v`

**Step 1: Run the full targeted Python verification slice**

Run:

```bash
pytest \
  tests/unit_tests/persistence/test_execution_fill_sqlite.py \
  tests/unit_tests/persistence/test_execution_fill_persistence_actor.py \
  tests/unit_tests/persistence/test_order_action_sqlite.py \
  tests/unit_tests/persistence/test_order_action_persistence_actor.py \
  tests/unit_tests/persistence/test_telemetry_shipper.py \
  tests/unit_tests/flux/strategies/shared/test_trades.py \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/flux/api/test_tokenmm_compat.py \
  tests/unit_tests/flux/api/test_socketio_tokenmm.py \
  tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py -v
```

**Step 2: Run the full targeted Fluxboard verification slice**

Run:

```bash
cd fluxboard && pnpm test:run \
  api.flux.test.ts \
  Trades.test.tsx \
  Trades.mobile.test.tsx \
  __tests__/trades-integration.test.tsx \
  Trades.recovery.test.tsx \
  components/trades/rollups.test.ts
pytest \
  tests/unit_tests/research/test_telemetry_helpers.py \
  tests/unit_tests/ops/test_tokenmm_markouts_exporter.py -v
```

**Step 3: Spot-check the intended OKX semantics from the final contract**

Verify the final evidence proves:
- config `qty=1000` still submits `100` OKX contracts internally
- persistence stores `*_venue=100`, `*_base=1000`, and conversion provenance
- shared trade rows emit `qty=100`, `qty_base=1000`, and `qty_venue=100`
- TokenMM trade API emits `qty=1000` and `qty_venue=100`
- Fluxboard, research helpers, and exporters use `1000` as the primary operator quantity

**Step 4: Commit the final verification-only diff if needed**

If verification required fixture or snapshot updates, commit them; otherwise skip.

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

Plan complete and saved to `docs/plans/2026-03-23-tokenmm-operator-qty-contract.md`. Two execution options:

**1. Subagent-Driven (this session)** - I orchestrate fresh subagent lanes, use spec-first review, and parallelize only when task ownership is disjoint

**2. Separate Session (checkpointed)** - Open new session with executing-plans, batch execution with human checkpoints

**Which approach?**
