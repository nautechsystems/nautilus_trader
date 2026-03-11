# Equities MakerV4 Review Follow-Ups Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Close the concrete Makerv4/equities review gaps so the live equities path is truthful, hedge-capable, route-safe, and operationally diagnosable without changing `/equities`, `profile=equities`, or `portfolio=equities`.

**Architecture:** Keep the current Makerv4/equities rollout as the base, but finish the missing runtime wiring instead of layering more UI or deploy affordances on top of partial behavior. The highest-risk gaps are in the strategy/runtime contract: real hedge execution hooks, execution enablement truth, route/fee telemetry, fail-fast API and bridge scoping, and dual-venue balances. Prefer tightening or deleting misleading surfaces over keeping knobs that do nothing.

**Tech Stack:** Python (Flux strategies/runners/API), Nautilus Trader strategy lifecycle and adapters, Redis, TOML deploy config, shell deploy scripts, React/TypeScript Fluxboard, pytest, Vitest.

## Review Findings This Plan Closes

1. MakerV4 strategy is still a publisher shell, not a fully wired trading strategy.
2. Runner submission/claim filters exclude the IBKR hedge instrument.
3. `--enable-execution` is ineffective against checked-in `enable_execution = false`.
4. Several Makerv4 params are exposed but unused.
5. MakerV4 signal telemetry fields are defined in the contract but not populated.
6. Hedge route and hedge leg identity are hardcoded/mirrored, so the UI cannot verify the real route.
7. Equities still claims `/tokenm`.
8. Equities bridge falls back to `identity.strategy_id` and can quietly consume nothing.
9. IBKR balances/positions are still not wired into the equities balances surface.
10. Local secret/bootstrap handling still omits `TRADE_XYZ_VAULT_ADDRESS` and does not validate IBKR readiness.

## Scope Rules

1. Keep `/equities`, `profile=equities`, and `portfolio=equities` stable.
2. Keep Makerv4 as the only active equities strategy family; do not add speculative mixed-version support.
3. Reuse and extract shared Makerv3 utilities only where Makerv4 now duplicates concrete logic.
4. Prefer fail-fast behavior over silent fallback for route, allowlist, and strategy-family mismatches.
5. Treat route visibility, fee visibility, and balance visibility as operator-facing contract work, not optional polish.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | completed | main | Tasks 1-8 complete. Final matrix passed (`163 pytest`, `23 vitest` on runnable files). One live residual remains documented: current host IBKR gateway handshake failure prevents live dual-venue balances on this box. |
| Task 1: Wire Real MakerV4 Hedge Lifecycle | completed | main | Lifecycle `on_order_filled` path and managed-order reporting verified locally; targeted Makerv4 pytest slice `17 passed` |
| Task 2: Fix Hedge Authorization And Execution Intent | completed | main | Runner override/dual-instrument contract verified in-worktree; explicit stack and TOML execution contract text added; local Task 2 pytest slice `30 passed` |
| Task 3: Make Makerv4 Params Truthful | completed | main | Instant-hedge and hedge-style controls fail closed in runtime, IOC cross is capped in pricing, dotted-symbol instrument mapping is covered, and targeted Task 3 slices passed (`25 pytest`, `9 vitest`) |
| Task 4: Populate Route And Spread Telemetry | completed | main | Distinct hedge/ref identity, route-aware telemetry, half-spread/effective spread math, and Fluxboard Hedge latency rendering verified (`50 pytest`, `3 vitest`) |
| Task 5: Fail Fast On API And Bridge Drift | completed | main | Removed equities `/tokenm` alias, enforced explicit strategy metadata/param-set consistency, and made bridge allowlists mandatory (`21 pytest`, `11 pytest`) |
| Task 6: Implement Dual-Venue Balances | completed | main | Added Makerv4 supplemental IBKR balance snapshot hook, preserved explicit IBKR account venue in flattening, attached runner provider, and verified Task 6 slice (`59 pytest`, `11 vitest`) |
| Task 7: Harden Deploy And Secret Contracts | completed | main | Local stack now allowlists `TRADE_XYZ_VAULT_ADDRESS`, validates IBKR docker gateway creds, and docs/env examples match; Task 7 slice passed (`14 pytest`, `bash -n`) |
| Task 8: Docs, Verification, And Rollback Record | completed | main | Wrote review/rollback docs, ran final backend matrix (`163 pytest`), ran Fluxboard runnable slice (`23 vitest`), and recorded the live IBKR handshake residual from host checks |

---

### Task 1: Wire Real MakerV4 Hedge Lifecycle

**Files:**
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv4/managed_orders.py`
- Modify: `systems/flux/flux/strategies/makerv4/wire.py`
- Modify: `systems/flux/flux/strategies/makerv4/publisher.py`
- Modify: `systems/flux/flux/strategies/shared/publisher_common.py` only if a shared helper becomes necessary
- Test: `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py`

**Step 1: Write the failing tests**

Add tests that prove:
1. a maker fill received through the Nautilus lifecycle creates one hedge request without calling helper-only methods directly
2. duplicate maker fills are ignored through the live event path
3. partial hedge fills leave pending state and make the strategy untradeable
4. restart/restore preserves pending hedge state and does not double-submit on reconnect
5. state payload exposes non-zero managed order counts when a hedge is pending

**Step 2: Run test to verify it fails**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py
```

Expected: FAIL because the current strategy does not hook the real order/fill lifecycle and still reports `managed_orders = 0`.

**Step 3: Write minimal implementation**

Implement the live path instead of helper-only plumbing:
1. hook the appropriate Nautilus order/fill callbacks in `MakerV4Strategy`
2. translate maker fills into hedge intents inside the strategy itself
3. persist pending hedge state and expose it through state payloads
4. only extract a shared helper if Makerv3 and Makerv4 truly duplicate the same state-publish logic

**Step 4: Run test to verify it passes**

Run the same pytest command plus:

```bash
uv run --group test pytest -q tests/unit_tests/flux/strategies/makerv4/test_observability_and_exports.py
```

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/makerv4/strategy.py \
  systems/flux/flux/strategies/makerv4/managed_orders.py \
  systems/flux/flux/strategies/makerv4/wire.py \
  systems/flux/flux/strategies/makerv4/publisher.py \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py
git commit -m "feat(equities): wire makerv4 hedge lifecycle"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Fix Hedge Authorization And Execution Intent

**Files:**
- Modify: `systems/flux/flux/runners/equities/run_node.py`
- Modify: `deploy/equities/strategies/aapl_tradexyz_makerv4.toml`
- Modify: `deploy/equities/strategies/equities.strategy.template.toml`
- Modify: `ops/scripts/deploy/equities_stack.sh`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_node.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`

**Step 1: Write the failing tests**

Add tests that prove:
1. `--enable-execution` is authoritative when explicitly requested for the local stack or systemd-generated node command
2. `allowed_submit_instrument_ids` includes both maker and hedge instruments for Makerv4
3. `external_order_claims` includes the hedge instrument or an equivalent explicit hedge-claim path
4. checked-in Makerv4 strategy config and stack script agree on the execution contract instead of working against each other

**Step 2: Run test to verify it fails**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py
```

Expected: FAIL on the current `enable_execution` precedence and hedge-authorization assumptions.

**Step 3: Write minimal implementation**

Choose one explicit contract and make every layer honor it:
1. if CLI `--enable-execution` stays, it must override TOML `false`
2. if TOML is the only source of truth, delete the CLI knob and remove the stack-script affordance
3. authorize the IBKR hedge instrument in the strategy config passed to Nautilus
4. document the chosen contract in the checked-in canary TOMLs and the stack launcher

Recommendation: keep the CLI/systemd opt-in and make it truthfully override TOML for local/runtime enablement.

**Step 4: Run test to verify it passes**

Run the same pytest command and then:

```bash
uv run --group test pytest -q tests/integration_tests/live/test_live_node.py -k makerv4
```

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/runners/equities/run_node.py \
  deploy/equities/strategies/aapl_tradexyz_makerv4.toml \
  deploy/equities/strategies/equities.strategy.template.toml \
  ops/scripts/deploy/equities_stack.sh \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py
git commit -m "fix(equities): align makerv4 execution intent and hedge authorization"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Make Makerv4 Params Truthful

**Files:**
- Modify: `systems/flux/flux/strategies/makerv4/runtime_params.py`
- Modify: `systems/flux/flux/strategies/makerv4/pricing.py`
- Modify: `systems/flux/flux/strategies/makerv4/fees.py`
- Modify: `systems/flux/flux/strategies/makerv4/instruments.py`
- Modify: `fluxboard/config/paramsProfiles.ts`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_pricing.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_instruments.py`
- Test: `fluxboard/__tests__/config/paramsProfiles.test.ts`

**Step 1: Write the failing tests**

Add tests that prove:
1. every Makerv4 param exposed in Fluxboard is either implemented or intentionally removed
2. `hedge_ioc_max_cross_bps` caps the IOC cross distance
3. `instant_hedge_enabled` genuinely suppresses hedge creation when false
4. unsupported fee-source selections fail fast instead of silently showing a control that does nothing
5. equity symbol mapping supports the practical symbol set you intend to trade, including punctuation cases if required

**Step 2: Run test to verify it fails**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py \
  tests/unit_tests/flux/strategies/makerv4/test_pricing.py \
  tests/unit_tests/flux/strategies/makerv4/test_instruments.py
cd fluxboard && pnpm vitest run __tests__/config/paramsProfiles.test.ts
```

Expected: FAIL because several current Makerv4 params are UI-visible but unused in the runtime.

**Step 3: Write minimal implementation**

Implement or delete misleading controls:
1. keep `instant_hedge_enabled`, `hedge_style`, and `hedge_ioc_max_cross_bps` only if they drive runtime behavior
2. keep fee-source controls only if the runtime enforces them
3. remove any Makerv4-only param from Fluxboard ordering if it remains intentionally unsupported
4. extend ticker mapping only as far as current equities coverage requires

**Step 4: Run test to verify it passes**

Run the same pytest and Vitest commands.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/makerv4/runtime_params.py \
  systems/flux/flux/strategies/makerv4/pricing.py \
  systems/flux/flux/strategies/makerv4/fees.py \
  systems/flux/flux/strategies/makerv4/instruments.py \
  fluxboard/config/paramsProfiles.ts \
  tests/unit_tests/flux/strategies/makerv4/test_runtime_params.py \
  tests/unit_tests/flux/strategies/makerv4/test_pricing.py \
  tests/unit_tests/flux/strategies/makerv4/test_instruments.py \
  fluxboard/__tests__/config/paramsProfiles.test.ts
git commit -m "fix(equities): make makerv4 params truthful"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Populate Route And Spread Telemetry

**Files:**
- Modify: `systems/flux/flux/strategies/shared/quote_snapshot.py`
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `systems/flux/flux/api/_payloads_signals.py`
- Modify: `fluxboard/components/domain/signal/MakerV4SignalTable.tsx`
- Modify: `fluxboard/types.ts`
- Test: `tests/unit_tests/flux/api/test_payloads.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py`
- Test: `fluxboard/tests/signal/MakerV4SignalTable.test.tsx`

**Step 1: Write the failing tests**

Add tests that prove:
1. `effective_spread_bps`, `quoted_spread_bps`, `expected_maker_fee_bps`, `fee_snapshot_age_s`, `hedge_latency_ms`, and `hedge_slippage_bps_vs_mid` are present when the runtime has enough information
2. `hedge_leg` is not just a clone of `ref_leg` when route metadata differs
3. route metadata shows the actual configured/used route, not hardcoded `SMART`
4. the UI displays route and spread fields without placeholder-only behavior

**Step 2: Run test to verify it fails**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py
cd fluxboard && pnpm vitest run tests/signal/MakerV4SignalTable.test.tsx
```

Expected: FAIL because the current Makerv4 snapshot does not populate most of the advertised telemetry.

**Step 3: Write minimal implementation**

1. compute and publish actual quoted/effective spread fields from the Makerv4 pricing path
2. publish maker-fee and hedge-fee values actually used for pricing
3. carry route identity explicitly, with room for `SMART` vs `BLUEOCEAN`
4. separate reference-leg identity from executed hedge-leg identity

**Step 4: Run test to verify it passes**

Run the same pytest and Vitest commands.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/shared/quote_snapshot.py \
  systems/flux/flux/strategies/makerv4/strategy.py \
  systems/flux/flux/api/_payloads_signals.py \
  fluxboard/components/domain/signal/MakerV4SignalTable.tsx \
  fluxboard/types.ts \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/strategies/makerv4/test_publisher_contract.py \
  fluxboard/tests/signal/MakerV4SignalTable.test.tsx
git commit -m "feat(equities): publish truthful makerv4 route and spread telemetry"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 5: Fail Fast On API And Bridge Drift

**Files:**
- Modify: `systems/flux/flux/runners/shared/strategy_set.py`
- Modify: `systems/flux/flux/runners/equities/run_api.py`
- Modify: `systems/flux/flux/runners/equities/run_bridge.py`
- Modify: `deploy/equities/equities.live.toml`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_api.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_bridge.py`

**Step 1: Write the failing tests**

Add tests that prove:
1. equities no longer answers `/tokenm`
2. missing or invalid equities `api.strategy_class` fails fast instead of silently defaulting to Makerv3 metadata
3. bridge requires `api.equities_strategy_ids` for the equities surface and does not silently fall back to `identity.strategy_id`

**Step 2: Run test to verify it fails**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/examples/strategies/test_equities_run_bridge.py
```

Expected: FAIL on current aliasing and fallback behavior.

**Step 3: Write minimal implementation**

1. remove `/tokenm` from the equities descriptor and redirect wiring
2. treat `api.strategy_class` / `api.param_set` drift as startup errors on equities
3. require explicit equities allowlists for the bridge, with a clear startup error if they are missing
4. keep the live canary config explicit and minimal

**Step 4: Run test to verify it passes**

Run the same pytest command and a focused live API smoke:

```bash
uv run --group test pytest -q tests/unit_tests/flux/api/test_equities_profile_contract.py -k equities
```

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/runners/shared/strategy_set.py \
  systems/flux/flux/runners/equities/run_api.py \
  systems/flux/flux/runners/equities/run_bridge.py \
  deploy/equities/equities.live.toml \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/examples/strategies/test_equities_run_bridge.py
git commit -m "fix(equities): fail fast on api and bridge drift"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 6: Implement Dual-Venue Balances

**Files:**
- Modify: `systems/flux/flux/runners/live/venues.py`
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv3/publisher.py` only if a shared balance publisher helper is required
- Modify: `systems/flux/flux/api/_payloads_balances.py`
- Modify: `fluxboard/Balances.tsx` only if the API contract needs small rendering support after backend is fixed
- Modify: `deploy/equities/strategies/aapl_tradexyz_makerv4.toml`
- Test: `tests/unit_tests/flux/api/test_equities_profile_contract.py`
- Test: `tests/unit_tests/examples/strategies/test_live_venue_registry.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`
- Test: `fluxboard/Balances.test.tsx`

**Step 1: Write the failing tests**

Add tests that prove:
1. IBKR account cash and positions can be collected for a data-only reference venue
2. Hyperliquid and IBKR rows both appear in the equities balances payload
3. shared IBKR account cash is deduped or explicitly marked as shared instead of being repeated per strategy
4. Fluxboard balances renders the combined rows without double-counting net equity

**Step 2: Run test to verify it fails**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  tests/unit_tests/examples/strategies/test_live_venue_registry.py \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py
cd fluxboard && pnpm vitest run Balances.test.tsx
```

Expected: FAIL because current equities balances can only surface Hyperliquid account state.

**Step 3: Write minimal implementation**

1. add a dedicated IBKR account-state ingestion path for equities; do not fake this through execution enablement if IBKR remains data-only
2. publish the IBKR account/position snapshot alongside Hyperliquid balances
3. preserve shared-account semantics so cash is not repeated per strategy
4. keep portfolio aggregation profile-stable as `equities`

**Step 4: Run test to verify it passes**

Run the same pytest and Vitest commands.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/runners/live/venues.py \
  systems/flux/flux/strategies/makerv4/strategy.py \
  systems/flux/flux/api/_payloads_balances.py \
  fluxboard/Balances.tsx \
  deploy/equities/strategies/aapl_tradexyz_makerv4.toml \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  tests/unit_tests/examples/strategies/test_live_venue_registry.py \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  fluxboard/Balances.test.tsx
git commit -m "feat(equities): wire dual-venue balances for makerv4"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 7: Harden Deploy And Secret Contracts

**Files:**
- Modify: `ops/scripts/deploy/equities_stack.sh`
- Modify: `deploy/equities/README.md`
- Modify: `deploy/equities/systemd/common.env.example`
- Modify: `deploy/equities/strategies/README.md`
- Test: `tests/unit_tests/examples/strategies/test_equities_stack_contract.py`

**Step 1: Write the failing tests**

Add tests that prove:
1. AWS secret loading supports `TRADE_XYZ_VAULT_ADDRESS`
2. local stack validation checks the actual IBKR requirements for the active dockerized gateway contract
3. README examples match the loader and validator behavior

**Step 2: Run test to verify it fails**

Run:

```bash
uv run --group test pytest -q tests/unit_tests/examples/strategies/test_equities_stack_contract.py
```

Expected: FAIL on the current vault-address omission and incomplete IBKR validation path.

**Step 3: Write minimal implementation**

1. allowlist `TRADE_XYZ_VAULT_ADDRESS` in the secret loader
2. validate whichever IBKR env vars the active local/dockerized path truly requires
3. update docs so “local smoke” does not claim readiness when the reference leg cannot come up

**Step 4: Run test to verify it passes**

Run the same pytest command and:

```bash
bash -n ops/scripts/deploy/equities_stack.sh
```

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  ops/scripts/deploy/equities_stack.sh \
  deploy/equities/README.md \
  deploy/equities/systemd/common.env.example \
  deploy/equities/strategies/README.md \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py
git commit -m "fix(equities): harden local stack secrets and ibkr validation"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 8: Docs, Verification, And Rollback Record

**Files:**
- Modify: `docs/plans/2026-03-07-equities-makerv4.md`
- Create or Modify: `docs/reviews/2026-03-09-equities-makerv4-review-followups-review.md`
- Modify: `deploy/equities/README.md`
- Modify: `fluxboard/docs/equities_contract.md`

**Step 1: Write the failing checklist**

Create a final checklist that requires:
1. no `/tokenm` equities alias
2. truthful execution enablement behavior
3. real Makerv4 hedge lifecycle
4. truthful route/spread telemetry
5. dual-venue balances rows
6. vault-address contract coverage
7. updated rollback steps for disabling Makerv4 or reverting to the disabled Makerv3 rollback file

**Step 2: Run verification matrix**

Run:

```bash
uv run --group test pytest -q \
  tests/unit_tests/examples/strategies/test_equities_run_api.py \
  tests/unit_tests/examples/strategies/test_equities_run_bridge.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/strategies/makerv4
cd fluxboard && pnpm vitest run \
  tests/signal/MakerV4SignalTable.test.tsx \
  Balances.test.tsx \
  __tests__/config/paramsProfiles.test.ts \
  __tests__/panels/signal.test.tsx
```

Expected: PASS.

**Step 3: Update docs**

1. record exactly which review findings were fixed
2. note any residuals that remain real after verification
3. add one rollback paragraph covering clean Makerv4 disablement and emergency Makerv3 re-enable
4. keep the original 2026-03-07 plan as rollout history and log follow-up completion there

**Step 4: Run final live checks**

Run targeted live checks on the current equities host:

```bash
curl -fsS 'http://127.0.0.1:5022/api/v1/signals?profile=equities'
curl -fsS 'http://127.0.0.1:5022/api/v1/balances?profile=equities'
journalctl -u flux@equities-node-aapl_tradexyz_makerv4.service -n 200 --no-pager
journalctl -u flux@equities-bridge.service -n 200 --no-pager
```

Expected:
1. no equities `/tokenm` alias
2. MakerV4 row includes real route/spread telemetry
3. balances include both venue families or a documented, explicit residual if IBKR is out-of-hours
4. no silent bridge empty-state fallback

**Step 5: Commit**

```bash
git add \
  docs/plans/2026-03-07-equities-makerv4.md \
  docs/plans/2026-03-09-equities-makerv4-review-followups.md \
  docs/reviews/2026-03-09-equities-makerv4-review-followups-review.md \
  deploy/equities/README.md \
  fluxboard/docs/equities_contract.md
git commit -m "docs(equities): record makerv4 review follow-up verification"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.
