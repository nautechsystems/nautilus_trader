# Base Unit Risk And Balance Normalization Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make strategy risk, balances, and portfolio inventory operate on explicit base-asset exposure while preserving NautilusTrader's existing venue-native quantity semantics for orders, fills, and positions.

**Architecture:** Do not redefine `Quantity` or `Position.quantity` to mean base units. In NautilusTrader today, domain quantities are venue/native size and interact with `multiplier` for notional and PnL. The correct abstraction is a new explicit base-exposure calculation path, separate from the existing quote-to-base helper. Strategy risk and observability will consume `*_base` fields derived from that path, while execution continues to use `*_venue`.

**Tech Stack:** NautilusTrader core instrument/model surfaces (Python and Rust), OKX adapter metadata, Flux MakerV3 strategy and portfolio inventory, Flux API payloads, Fluxboard contracts/docs, pytest, cargo test.

---

## Progress Tracker

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Task 1: Codify Quantity Semantics And Contracts | completed | codex | Docs/contracts updated and verified with `rg -n "local_qty|position_qty|order_qty|qty_conversion" docs fluxboard/docs systems/flux/docs`. |
| Task 2: Add Core Base-Exposure Calculation API | completed | codex | Added explicit base-exposure APIs in Python/Rust. Verified with `pytest tests/unit_tests/model/test_instrument.py -k "base_exposure" -v` and `cargo test -p nautilus-model calculate_base_exposure -- --nocapture`. |
| Task 3: Surface Quantity Unit Metadata Needed For Exact Conversion | completed | codex | OKX metadata now preserves explicit unsupported states without dropping raw fields. Verified with swap parser `16 passed`, futures parser `4 passed`, missing-`lot_sz` helper `1 passed`, and non-`unsupported` incomplete-helper guard `3 passed`. |
| Task 4: Add Flux Quantity Normalization Wrapper | completed | codex | Added `flux.common.quantity_units` with explicit `QuantityExposure`, venue/base conversions, and degraded statuses. Verified with `pytest tests/unit_tests/flux/common/test_quantity_units.py -v` and `pytest tests/unit_tests/flux/common/test_quantity_units.py tests/unit_tests/flux/common/test_portfolio_inventory.py -v`. |
| Task 5: Move MakerV3 Risk And Portfolio Inventory To Explicit Base Exposure | completed | codex | Closed spec-review gaps by degrading skew when base exposure is unavailable and adding venue/debug fields to shared inventory components. Verified with `pytest tests/unit_tests/flux/common/test_quantity_units.py tests/unit_tests/flux/common/test_portfolio_inventory.py tests/unit_tests/flux/common/test_portfolio_snapshot.py tests/unit_tests/flux/strategies/makerv3/test_inventory.py tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py -v` and the combined `80 passed` Flux suite. Async spec re-review was requested but did not return before timeout. |
| Task 6: Add Explicit Order Quantity Units For Strategy Config | completed | codex | Added explicit `qty_unit` config, base-to-venue order conversion, runner plumbing, and explicit deploy TOMLs. Verified with `pytest tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py -v` (`13 passed`) and `pytest tests/unit_tests/flux/strategies/makerv3/test_order_safety.py -v` (`10 passed`). |
| Task 7: Publish Dual-Unit Observability And Balances Payloads | completed | codex | Verified dual-unit publisher/API payload contracts and base-first aliases with `pytest tests/unit_tests/flux/api/test_payloads.py -v` (`43 passed`), `pytest tests/unit_tests/flux/api/test_app.py -v` (`62 passed`), and `pytest tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -v` (`18 passed`). Async spec/quality reviewer threads timed out without findings. |
| Task 8: Add Startup Guardrails And Manual Verification | completed | codex | Added runner qty-unit guardrails, derivative startup quantity logging, README operator guidance, and fixed follow-up reviewer regressions in portfolio snapshot/API/publisher alias handling. Verified with targeted Python suites (`180 passed`, `49 passed`) plus `cargo test -p nautilus-model` and `cargo test -p nautilus-okx`. Broad `pytest tests/unit_tests/flux -q` remains red on 10 unrelated pre-existing failures in `tests/unit_tests/flux/bridge/test_handlers.py` and `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`. |

---

### Design Decision

**Keep existing Nautilus core semantics:**

- `Order.quantity`, `Fill.last_qty`, `Position.quantity`, and `Position.signed_qty` remain venue/native size.
- `multiplier` continues to represent the contract/share/lot scaling already used by notional and PnL.
- The existing `calculate_base_quantity(quantity, last_px)` remains quote-size conversion and must not be repurposed.

**Add a new explicit abstraction:**

- `venue_qty`: exchange-native size used for execution and reconciliation.
- `base_qty`: normalized exposure in base asset units used for strategy risk, balances, and portfolio inventory.
- `qty_conversion_status`: `identity`, `exact_multiplier`, `price_based`, `unsupported`, `missing_metadata`, `missing_price`, `non_integral_venue_qty`.
- `qty_conversion_source`: short explanation of which rule produced the conversion.

This design aligns with current core model behavior instead of fighting it, and generalizes beyond OKX.

### Task 1: Codify Quantity Semantics And Contracts

**Files:**
- Create: `docs/architecture/quantity-units.md`
- Modify: `fluxboard/docs/tokenmm_contract.md`
- Modify: `fluxboard/docs/tokenmm_socket_contract.md`

**Step 1: Write the doc checklist**

Document these invariants:

- venue quantities are canonical for execution
- base quantities are canonical for strategy exposure/risk
- payloads must never expose ambiguous risk-facing `qty` fields
- existing `quantity` fields on Nautilus positions/orders remain venue-native

**Step 2: Update external contract naming**

Target names:

- `position_qty_venue`
- `position_qty_base`
- `local_qty_venue`
- `local_qty_base`
- `order_qty_venue`
- `order_qty_base`
- `qty_conversion_status`
- `qty_conversion_source`

**Step 3: Verify docs**

Run:
```bash
rg -n "local_qty|position_qty|order_qty|qty_conversion" docs fluxboard/docs systems/flux/docs
```

Expected: docs explicitly distinguish venue vs base semantics.

### Task 2: Add Core Base-Exposure Calculation API

**Files:**
- Modify: `nautilus_trader/model/instruments/base.pyx`
- Modify: `nautilus_trader/model/instruments/base.pxd`
- Modify: `crates/model/src/instruments/mod.rs`
- Modify: `crates/model/src/instruments/any.rs` if needed
- Test: `tests/unit_tests/model/test_instrument.py`
- Test: `crates/model/src/instruments/mod.rs`

**Step 1: Write failing tests**

Add focused tests proving a new helper behaves correctly for venue/native quantity:

- spot or equity: `base_qty == venue_qty`
- linear contract with base-denominated multiplier: `base_qty = venue_qty * multiplier`
- inverse contract: `base_qty = venue_qty * multiplier / price`
- missing price for price-based conversion returns explicit failure / unsupported result

**Step 2: Implement a new explicit API**

Add a new method with a name that does not collide with quote-size conversion. Recommended shape:

- `try_calculate_base_exposure_qty(venue_qty, last_price=None)`
- `calculate_base_exposure_qty(...)` wrapper if a throwing convenience API is needed

Do **not** modify existing `calculate_base_quantity(...)` semantics.

Return a structured result or error path that makes unsupported/missing-price cases explicit.

**Step 3: Verify tests**

Run:
```bash
pytest tests/unit_tests/model/test_instrument.py -k "base_exposure" -v
cargo test -p nautilus-model calculate_base_exposure -- --nocapture
```

Expected: PASS.

### Task 3: Surface Quantity Unit Metadata Needed For Exact Conversion

**Files:**
- Modify: `crates/adapters/okx/src/common/parse.rs`
- Modify: `crates/adapters/okx/src/common/models.rs` if needed
- Modify: relevant instrument model/wrapper types only if required to carry the metadata cleanly
- Test: `crates/adapters/okx/src/common/parse.rs`

**Step 1: Write failing adapter tests**

Assert that parsed swap instruments preserve enough metadata to decide whether base exposure is:

- identity
- exact via multiplier
- price-based
- unsupported

For OKX this specifically means preserving:

- `ctVal`
- `ctValCcy`
- `ctType`
- `lotSz`

**Step 2: Implement clean metadata surfacing**

Do not hide this in string parsing scattered through Flux. Put it where instrument math belongs.

If current instrument types cannot express the needed metadata, add a small explicit field or helper surface rather than an untyped bag.

**Step 3: Verify tests**

Run:
```bash
cargo test -p nautilus-okx test_parse_swap_instrument -- --nocapture
```

Expected: PASS with assertions covering quantity-unit metadata.

### Task 4: Add Flux Quantity Normalization Wrapper

**Files:**
- Create: `systems/flux/flux/common/quantity_units.py`
- Test: `tests/unit_tests/flux/common/test_quantity_units.py`

**Step 1: Write failing tests**

Cover:

- conversion from instrument + `venue_qty` to `base_qty`
- propagation of `qty_conversion_status` and `qty_conversion_source`
- unsupported or missing-price cases degrade cleanly

This wrapper should be thin. It should orchestrate core instrument math plus adapter metadata, not reimplement pricing rules ad hoc.

**Step 2: Implement wrapper types**

Recommended minimal surface:

- `QuantityExposure`
- `exposure_from_venue_qty(instrument, venue_qty, last_px=None)`
- `venue_qty_from_base_qty(instrument, base_qty, last_px=None)`

The wrapper should return both the original venue quantity and normalized base quantity.

**Step 3: Verify tests**

Run:
```bash
pytest tests/unit_tests/flux/common/test_quantity_units.py -v
```

Expected: PASS.

### Task 5: Move MakerV3 Risk And Portfolio Inventory To Explicit Base Exposure

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv3/inventory.py`
- Modify: `systems/flux/flux/common/portfolio_inventory.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_inventory.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py`
- Test: `tests/unit_tests/flux/common/test_portfolio_inventory.py`

**Step 1: Write failing tests**

Cover:

- local maker risk uses `local_qty_base`
- shared portfolio inventory aggregates `local_qty_base`
- skew calculations use base exposure, not raw venue contracts
- unsupported conversions block/degrade rather than silently using venue qty

**Step 2: Implement explicit fields**

Replace ambiguous risk-facing semantics with explicit names:

- `local_position_qty_venue`
- `local_position_qty_base`
- `global_position_qty_venue`
- `global_position_qty_base`
- `inventory_qty_base`

Keep venue-native fields available for debug and reconciliation only.

**Step 3: Verify tests**

Run:
```bash
pytest tests/unit_tests/flux/strategies/makerv3/test_inventory.py -v
pytest tests/unit_tests/flux/strategies/makerv3/test_strategy_lifecycle.py -v
pytest tests/unit_tests/flux/common/test_portfolio_inventory.py -v
```

Expected: PASS.

### Task 6: Add Explicit Order Quantity Units For Strategy Config

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv3/runtime_params.py`
- Modify: tokenmm runner/config plumbing as needed
- Modify: `deploy/tokenmm/strategies/tokenmm.strategy.template.toml`
- Modify: `deploy/tokenmm/strategies/plumeusdt_okx_perp_makerv3.toml`
- Test: relevant MakerV3 strategy/runtime param tests

**Step 1: Write failing tests**

Add tests proving:

- `qty_unit = "venue"` preserves current behavior
- `qty_unit = "base"` converts desired base exposure into a venue/native order size before order creation
- non-integral native quantities fail fast with a clear error and alert

**Step 2: Implement strategy-level conversion**

Perform the conversion before constructing Nautilus `Quantity`.

That keeps execution clients unchanged and aligned with existing Nautilus order semantics.

**Step 3: Verify tests**

Run:
```bash
pytest tests/unit_tests/flux/strategies/makerv3/test_runtime_params.py -v
pytest tests/unit_tests/flux/strategies/makerv3/test_order_safety.py -v
```

Expected: PASS.

### Task 7: Publish Dual-Unit Observability And Balances Payloads

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/publisher.py`
- Modify: payload builders under `systems/flux/flux/api/`
- Modify: compatibility shims only where needed
- Test: `tests/unit_tests/flux/api/test_payloads.py`
- Test: `tests/unit_tests/flux/api/test_app.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`

**Step 1: Write failing payload tests**

Assert payloads expose:

- normalized base exposure fields for balances and strategy risk
- venue-native fields for reconciliation/debugging
- conversion metadata fields

**Step 2: Implement payload updates**

UI default should consume `*_base`.

Retain temporary aliases only if necessary, and document them as compatibility debt.

Compatibility debt shipped in Task 7:

- Raw strategy balance snapshots still publish legacy `signed_qty` / `quantity` as venue-native aliases for compatibility with existing downstream consumers of the Redis snapshot. The canonical fields on that surface are `signed_qty_base` / `quantity_base` and `signed_qty_venue` / `quantity_venue`.
- Flux API `/balances` and `/signals` still expose legacy aliases such as `signed_qty`, `quantity`, `global_qty`, `local_qty`, `curr_qty`, and `global_qty_complete`. When explicit base fields exist, those aliases now mirror the base-exposure fields and must not diverge from them.
- Removal path: after Fluxboard and any other direct Redis/API consumers are migrated to explicit `*_base` / `*_venue` fields, delete the legacy aliases from publisher, balances payload assembly, and signal fallback payloads in one coordinated compatibility cleanup.

**Step 3: Verify tests**

Run:
```bash
pytest tests/unit_tests/flux/api/test_payloads.py -v
pytest tests/unit_tests/flux/api/test_app.py -v
pytest tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py -v
```

Expected: PASS.

### Task 8: Add Startup Guardrails And Manual Verification

**Files:**
- Modify: startup logging / config validation paths in tokenmm runners and strategy init
- Modify: `deploy/tokenmm/README.md`

**Step 1: Add guardrail logging**

At strategy startup for derivatives, log:

- instrument id
- raw venue position qty
- computed base exposure qty
- conversion status/source
- configured order qty and `qty_unit`

**Step 2: Add validation**

Reject or warn on:

- missing `qty_unit`
- unsupported base conversion for a strategy configured to risk in base units
- non-integral venue order sizes after base-to-venue conversion

**Step 3: End-to-end verification**

Run:
```bash
pytest tests/unit_tests/flux -q
cargo test -p nautilus-model
cargo test -p nautilus-okx
```

Expected: PASS.

**Step 4: Manual live checklist**

- query live instrument metadata
- query live venue position
- confirm `venue_qty -> base_qty` matches GUI exposure
- confirm a base-configured order converts to expected venue contracts

### Recommended Delivery Order

1. Codify the semantics and payload contract.
2. Add a new core base-exposure calculation API.
3. Surface exact quantity-unit metadata from adapters.
4. Add Flux normalization wrapper.
5. Move MakerV3 risk and portfolio inventory to `*_base`.
6. Add `qty_unit` config and order conversion.
7. Publish dual-unit payloads and startup guardrails.

### Explicit Non-Goals

- Do not redefine existing Nautilus `quantity` fields to base units.
- Do not change OKX execution clients to accept base units directly.
- Do not ship an OKX-only multiplier patch inside Flux strategy logic.

### Success Criteria

- A `343` contract OKX PLUME position still exists as `343` venue units in domain/debug surfaces.
- The same position is represented as `3430` base units everywhere strategy risk and balances care.
- Strategy config can explicitly choose `qty_unit = "base"` or `qty_unit = "venue"`.
- Unsupported conversions fail loudly and early instead of silently skewing risk.
