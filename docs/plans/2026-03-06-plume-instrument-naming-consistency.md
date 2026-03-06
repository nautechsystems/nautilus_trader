# PLUME Instrument Naming Consistency Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
>
> **For executing agent:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** Make PLUME spot and PLUME perp naming maximally consistent across internal contracts and operator views, so the system preserves instrument identity everywhere and only collapses to underlying-level labels in explicitly aggregate views.

**Architecture:**

1. Separate three concepts that are currently mixed together: instrument identity, inventory aggregation identity, and operator display label.
2. Derive canonical naming fields once in backend/common helpers, carry them through Redis/API payloads, and stop re-inferring them in Fluxboard.
3. Allow aggregate surfaces to intentionally group on underlying (`PLUME`) while Signal legs, Trades rows, and Balances child rows render instrument-level labels that preserve spot/perp distinctions.

**Tech Stack:** Python + Flask + Redis + Flux API/runner code, React 18 + TypeScript + Fluxboard.

---

## Review summary

### What is inconsistent today

1. Balances intentionally rewrites perp positions onto the base asset and Fluxboard then groups rows under `${canonical}_LOGICAL`, so spot cash and perp positions both scan as `PLUME`.
2. Trades normalizes `coin` from `symbol`/`instrument_id` by stripping quote and product suffixes, so `PLUMEUSDT.BINANCE_SPOT`, `PLUMEUSDT-SPOT.BYBIT`, and `PLUMEUSDT-LINEAR.BYBIT` all render as `PLUME`.
3. Signal legs render `exchange + coin` and currently use lossy leg identity (`contract_id` and `coin`) that hides product type.
4. The API/runner contract catalog and market-key model are still base/quote oriented in places, so same-venue spot/perp for the same pair are not first-class distinct contracts internally.

### What is already correct

1. Strategy ids are descriptive and already encode venue/product intent.
2. Raw `instrument_id` often exists at the producer edge and is the correct stable identity to preserve.
3. Underlying-only aggregation is legitimate for inventory/risk views, as long as it is explicit and does not leak into instrument-level surfaces.

---

## Options considered

### Option A: UI-only relabeling

- Add `spot` / `perp` badges in Fluxboard using heuristics from existing strings.
- Reject this as the primary fix because it leaves internal contract collisions intact and keeps every view dependent on slightly different fallback logic.

### Option B: Additive API naming overlay only

- Add explicit display fields in payloads but keep current catalog/key semantics unchanged.
- Better than Option A, but still incomplete because internal market identity remains ambiguous for same-venue spot/perp pairs.

### Option C: End-to-end identity hardening (recommended)

- Make instrument identity first-class in config, Redis/API helpers, and Fluxboard.
- Preserve aggregate `inventory_asset` / `base_asset` separately from instrument identity.
- Migrate operator views to render backend-derived naming fields instead of stripping labels client-side.

**Recommendation:** Execute Option C in additive phases, keeping existing fields as compatibility shims until all operator surfaces have switched to the new naming contract.

---

## Non-negotiable acceptance criteria

1. The system can represent same-venue `PLUME` spot and `PLUME` perp simultaneously without contract-catalog or market-key collisions.
2. Every Signal leg, Trades row, and Balances child row carries a stable instrument identity plus a display label that preserves product type.
3. Balances logical parents and risk underlyings may still show `PLUME`, but those views must be clearly aggregate-only and never reused as instrument identity.
4. Fluxboard filters expose distinct dimensions for `Underlying`, `Venue`, and `Market Type` instead of overloading `coin`.
5. CSV/export/debug surfaces include both the human label and the full canonical instrument identity.
6. Tests lock the PLUME case shown in the screenshot:
   - `PLUMEUSDT-SPOT.BYBIT`
   - `PLUMEUSDT-LINEAR.BYBIT`
   - `PLUME-USDT-SWAP.OKX`
   - `PLUMEUSDT.BINANCE_SPOT`

---

## Canonical naming contract

These fields should be the shared model across balances, trades, and signal payloads:

- `instrument_uid`: stable internal join key; recommended shape `venue_root:contract_type:instrument_id`
- `instrument_id`: native Nautilus instrument id, for example `PLUMEUSDT-LINEAR.BYBIT`
- `venue`: concrete venue code used in the surface, for example `BYBIT`, `BINANCE_SPOT`, `OKX`
- `venue_root`: operator-facing venue family, for example `bybit`, `binance`, `okx`
- `product_type`: `spot | perp`
- `contract_type`: `spot | linear | swap | inverse | cash`
- `raw_symbol`: venue-native tradable symbol, for example `PLUMEUSDT`, `PLUME-USDT-SWAP`
- `base_asset`: `PLUME`
- `quote_asset`: `USDT`
- `pair`: normalized pair, for example `PLUME/USDT`
- `inventory_asset`: underlying asset used for inventory/risk grouping, for example `PLUME`
- `display_name_short`: short label, for example `PLUME Spot`, `PLUME Perp`
- `display_name_long`: long label, for example `Bybit PLUME Perp`, `Binance PLUME Spot`

Rules:

1. `instrument_uid` or `instrument_id` is the primary join key.
2. `inventory_asset` may be used for aggregation only.
3. `coin`, `asset`, and `canonical` remain compatibility/aggregation fields and must not be treated as instrument identity by new code.
4. `contract_id` should remain as a compatibility alias only where current Signal/socket payloads require it; it must become product-aware and non-lossy if retained.

---

## View policy

### Signal

- Strategy row keeps full `strategy_id`.
- Leg labels render instrument-level naming:
  - `Bybit PLUME Perp`
  - `Binance PLUME Spot`
- Tooltips expose full `instrument_id`.

### Trades

- Primary trade label is instrument-level, not base-only.
- If venue is already a separate column, render `PLUME Perp` / `PLUME Spot`.
- If venue is hidden/collapsed, render `Bybit PLUME Perp` / `Binance PLUME Spot`.

### Balances

- Parent/logical rows stay underlying-based and explicitly marked as aggregate (`Logical`).
- Child rows render instrument-level labels for positions and venue/account labels for spot cash.
- Risk view stays underlying-based by design.

---

## Implementation plan

### Task 1: Freeze the naming contract and migration rules

**Files:**

- Modify: `docs/flux/api.md`
- Modify: `docs/flux/redis_schema.md`
- Modify: `docs/fluxboard/tokenmm_contract.md`
- Create or modify: `docs/fluxboard/tokenmm_naming.md`

**Steps:**

1. Document the new naming model and explicitly separate `instrument_id` from `inventory_asset`.
2. Define which views are aggregate-only (`Balances logical`, `Risk`) and which must remain instrument-level (`Signal`, `Trades`, `Balances child rows`).
3. Freeze additive compatibility rules:
   - old fields remain during migration
   - new fields become required for TokenMM payloads once Fluxboard switches
4. Document the exact PLUME spot/perp examples used as acceptance fixtures.

**Verification:**

- Confirm the docs no longer describe contract identity as only `exchange + base + quote`.

**Suggested commit:**

- `docs: freeze tokenmm instrument naming contract`

### Task 2: Make internal contract identity instrument-aware

**Files:**

- Modify: `nautilus_trader/flux/common/keys.py`
- Modify: `nautilus_trader/flux/api/app.py`
- Modify: `nautilus_trader/flux/api/payloads.py`
- Modify: `nautilus_trader/flux/runners/tokenmm/run_api.py`
- Modify: `deploy/tokenmm/tokenmm.live.toml`
- Test: `tests/unit_tests/flux/api/test_app.py`
- Test: `tests/unit_tests/flux/api/test_payloads.py`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`

**Steps:**

1. Replace base/quote-only market contract dedupe with an instrument-aware contract spec that can distinguish same-venue spot/perp.
2. Extend TokenMM contract catalog entries to carry explicit product metadata instead of only `exchange` + `symbol`.
3. Update Redis/API key-building helpers so same-venue `PLUME` spot and `PLUME` perp can coexist safely.
4. Keep backward-compatible reads only where necessary, but make new writes and new validation use the instrument-aware identity.
5. Add regression tests that prove `BYBIT` spot and `BYBIT` linear contracts for PLUME are both valid at the same time.

**Verification commands:**

```bash
pytest tests/unit_tests/flux/api/test_payloads.py -q
pytest tests/unit_tests/flux/api/test_app.py -q
pytest tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py -q
```

**Suggested commit:**

- `feat: make tokenmm contract identity instrument-aware`

### Task 3: Add canonical naming fields to balances, trades, and signal payloads

**Files:**

- Modify: `nautilus_trader/flux/api/payloads.py`
- Modify: `nautilus_trader/flux/strategies/makerv3/publisher.py`
- Modify: `nautilus_trader/flux/strategies/makerv3/inventory.py`
- Test: `tests/unit_tests/flux/api/test_payloads.py`
- Test: `tests/unit_tests/flux/api/test_app.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`

**Steps:**

1. Introduce a shared helper in backend/common code that derives the canonical naming fields from `instrument_id`, venue metadata, and product type.
2. Signal payload:
   - attach full naming fields to every leg
   - stop treating `coin` as the only operator-facing name
   - keep `legs_order` deterministic with product-aware identity
3. Trades payload:
   - guarantee pass-through of `instrument_id`
   - derive `display_name_short`, `display_name_long`, `base_asset`, and `product_type` server-side when upstream rows do not provide them
4. Balances payload:
   - keep `asset` / `coin` / `inventory_asset` for aggregation
   - add explicit instrument naming fields to position rows
   - add clear cash-vs-position naming semantics so a spot wallet row is not confused with a perp position row
5. Preserve compatibility fields for existing clients during rollout.

**Verification commands:**

```bash
pytest tests/unit_tests/flux/api/test_payloads.py -q
pytest tests/unit_tests/flux/api/test_app.py -q
pytest tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -q
```

**Suggested commit:**

- `feat: expose canonical instrument naming fields in flux payloads`

### Task 4: Switch Fluxboard types and normalization to the new naming contract

**Files:**

- Modify: `fluxboard/types.ts`
- Modify: `fluxboard/api.ts`
- Modify: `fluxboard/stores.ts`
- Test: `fluxboard/__tests__/api.test.ts`
- Test: `fluxboard/__tests__/stores-trades.test.ts`
- Test: `fluxboard/__tests__/trades-integration.test.tsx`

**Steps:**

1. Add the new naming fields to shared TypeScript types for balances, trades, and signal legs.
2. Remove lossy client-side inference as the primary path:
   - stop deriving display identity from stripped `coin`
   - use backend-provided `display_name_*`, `product_type`, and `instrument_id`
3. Keep old fallback heuristics only as temporary compatibility code paths, with explicit TODO markers for deletion after rollout.
4. Update store normalization so `coin` becomes an aggregate/filter hint instead of the primary display identity.

**Verification commands:**

```bash
pnpm --dir fluxboard test -- --run fluxboard/__tests__/api.test.ts
pnpm --dir fluxboard test -- --run fluxboard/__tests__/stores-trades.test.ts
pnpm --dir fluxboard test -- --run fluxboard/__tests__/trades-integration.test.tsx
```

**Suggested commit:**

- `feat: migrate fluxboard data normalization to canonical naming fields`

### Task 5: Update operator views and filters

**Files:**

- Modify: `fluxboard/components/domain/signal/SignalTable.tsx`
- Modify: `fluxboard/Trades.tsx`
- Modify: `fluxboard/components/trades/columns.tsx`
- Modify: `fluxboard/Balances.tsx`
- Modify: `fluxboard/components/shared/CoinCell.tsx`
- Modify: `fluxboard/components/balances/RiskTable.tsx`
- Test: `fluxboard/tests/signal/MakerV2Overlay.test.tsx`
- Test: `fluxboard/Balances.test.tsx`
- Test: `fluxboard/__tests__/panels/trades.behavior.test.tsx`
- Test: `fluxboard/__tests__/panels/signal.test.tsx`

**Steps:**

1. Signal:
   - render leg labels from canonical instrument naming fields
   - add `Market Type` as a filter dimension
   - keep strategy id unchanged
2. Trades:
   - render instrument-level labels in the coin/instrument column
   - add separate filters for `Underlying`, `Venue`, `Market Type`
   - include full `instrument_id` in tooltip/copy/export
3. Balances:
   - leave logical parents as `PLUME` aggregate rows
   - upgrade child rows so perp positions render as `PLUME Perp` and spot holdings render as `PLUME Spot` or equivalent venue-aware label
   - keep risk view underlying-only, but clearly position it as an aggregate exposure view
4. Update exports and CSV rows to include both display label and full instrument identity.

**Verification commands:**

```bash
pnpm --dir fluxboard test -- --run fluxboard/tests/signal/MakerV2Overlay.test.tsx
pnpm --dir fluxboard test -- --run fluxboard/Balances.test.tsx
pnpm --dir fluxboard test -- --run fluxboard/__tests__/panels/trades.behavior.test.tsx
pnpm --dir fluxboard test -- --run fluxboard/__tests__/panels/signal.test.tsx
```

**Suggested commit:**

- `feat: render instrument-level naming consistently across tokenmm views`

### Task 6: Remove dangerous lossy assumptions and lock invariants with regression tests

**Files:**

- Modify: `fluxboard/__tests__/api.test.ts`
- Modify: `fluxboard/__tests__/stores-trades.test.ts`
- Modify: `fluxboard/__tests__/trades-integration.test.tsx`
- Modify: `tests/unit_tests/flux/api/test_payloads.py`
- Modify: `tests/unit_tests/flux/api/test_app.py`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`

**Steps:**

1. Replace old expectations that assert `PLUMEUSDT-LINEAR.*` becomes display `PLUME` in instrument-level views.
2. Add golden tests proving:
   - same-venue spot/perp contracts survive the backend contract catalog
   - signal legs keep product identity
   - trades keep instrument identity during REST and socket normalization
   - balances logical parents aggregate intentionally while child rows remain instrument-specific
3. Add one end-to-end TokenMM naming fixture covering the exact production PLUME set.

**Verification commands:**

```bash
pytest tests/unit_tests/flux/api/test_payloads.py -q
pytest tests/unit_tests/flux/api/test_app.py -q
pytest tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py -q
pnpm --dir fluxboard test -- --run fluxboard/__tests__/api.test.ts
pnpm --dir fluxboard test -- --run fluxboard/__tests__/stores-trades.test.ts
pnpm --dir fluxboard test -- --run fluxboard/__tests__/trades-integration.test.tsx
```

**Suggested commit:**

- `test: lock tokenmm spot-perp naming invariants`

---

## Recommended rollout order

1. Land the internal contract and payload changes additively first.
2. Switch Fluxboard to prefer the new fields while retaining compatibility fallbacks.
3. Remove lossy display derivation once production payloads are confirmed stable.
4. Only then consider deprecating legacy fields or rekeying Signal/socket payloads more aggressively.

---

## Risks and mitigations

1. **Risk:** breaking existing TokenMM or socket clients by changing leg keys too early.
   - **Mitigation:** keep `contract_id` compatibility during the first migration wave and add new fields first.
2. **Risk:** balances/risk views accidentally stop aggregating on underlying.
   - **Mitigation:** preserve `inventory_asset` / `canonical` aggregate semantics explicitly and test them.
3. **Risk:** backend market-key migrations require data backfill or dual-read behavior.
   - **Mitigation:** make writes instrument-aware first, add compatibility reads where needed, and validate with a single PLUME staging profile before cleanup.

---

## Done criteria

This work is done when:

1. Operators can distinguish `PLUME` spot from `PLUME` perp at first glance in Signal, Trades, and Balances child rows.
2. Internal helpers and contract catalog can represent same-venue spot/perp without collision.
3. Aggregate-only views still show underlying exposure clearly and intentionally.
4. Regression tests cover the exact PLUME configuration currently deployed in TokenMM.

---

Plan complete and saved to `docs/plans/2026-03-06-plume-instrument-naming-consistency.md`. Two execution options:

**1. Subagent-Driven (this session)** - I dispatch fresh subagent per task, review between tasks, fast iteration

**2. Parallel Session (separate)** - Open new session with executing-plans, batch execution with checkpoints

**Which approach?**
