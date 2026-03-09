# Nautilus MakerV3 Single-Leg 3-Band Quoter MVP Implementation Plan (Fluxboard TokenMM UI)

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a NautilusTrader live POC that runs a single-leg MakerV3-style quoting strategy (`strategy_market` only) with 3 quote bands, and minimal Chainsaw wiring: params in, signals/state out, balances, trades, alerts, with **GUI provided by existing Fluxboard TokenMM surface** (no new UI).

**Architecture:** One Nautilus live `TradingNode` connects to Bybit (execution + market data) and Binance (market data only, for connector + bookbuilding validation). A single strategy manages post-only limit ladders on the `strategy_market` using 3 bands. The strategy publishes compact JSON events/state externally to a Redis stream (recommended: via Nautilus MessageBus backing with aggressive `types_filter` to avoid flooding Redis with market data). A small bridge consumes the stream and writes **FluxAPI-compatible Redis keys** (state/events/trades/alerts/balances/last:*/fvs.snapshot) so **Fluxboard TokenMM** can display the strategy via the existing Flask backend + Socket.IO.

**Tech Stack:** NautilusTrader Python API + Bybit/Binance adapters, Nautilus `MessageBusConfig` Redis Streams, Redis, small Python bridge (`redis-py`), existing Chainsaw `fluxapi` + `fluxboard` for UI.

---

## Dependency management (POC)

This POC includes one optional runtime dependency which is not guaranteed to be present in the NautilusTrader python environment:
1. `redis` (redis-py) for the bridge.

Pick one approach early and stick to it across agents:
1. Recommended (fast, low repo churn): run the bridge in a separate venv or in an existing environment which already has these deps (for example the Chainsaw environment), while keeping the code in `examples/live/poc/`.
1. Repo-contained (more churn): add a new dependency group (for example `[dependency-groups].poc`) and update lockfiles so `uv` installs are reproducible.

If you choose the recommended approach, document it in `examples/live/poc/README.md` and avoid changing NautilusTrader lockfiles for the POC.

## Scope (MVP)

1. Single strategy market only (`strategy_market`).
1. 3 quote bands on both sides (bid + ask), post-only limit orders.
1. Basic order lifecycle: place, replace, cancel, clear-on-off.
1. Params:
1. Read MakerV3-compatible Redis param keys: `strategy.<strategy_id>.<param>`.
1. Support hot updates by polling at a fixed interval (no fancy watchers).
1. Observability:
1. Emit state snapshots and per-event messages to Redis Streams (MessageBus backing).
1. Bridge emits FluxAPI/Fluxboard-compatible keys:
1. `maker_arb:<strategy_id>:state`
1. `maker_arb:<strategy_id>:events`
1. `trades.blotter` (rows on fills)
1. `alerts.blotter` (rows on alerts)
1. `balances.snapshot` and `balances` hash (rows for Bybit; optional Binance rows if connected)
1. `last:<exchange>:<symbol>` BBO snapshots for Fluxboard Market Data and Signal calculations
1. `fvs.snapshot` (Fluxboard Signal hard-requires non-empty FV snapshot to render any strategies)
1. Market data:
1. Bybit: L2 book building + trades for `strategy_market` (required).
1. Binance: market-data connector enabled and bookbuilding validated (required; not used by strategy logic).
1. Risk/circuit breaker:
1. Use Nautilus pre-trade checks and trading-state gating where possible.
1. Strategy-level gating is limited to `bot_on` and simple staleness logic.
1. GUI:
1. Fluxboard TokenMM surface must show: Signal, Params, Balances, Trades, Alerts.

## Explicit non-goals (MVP)

1. No hedge leg, no multi-instrument fair value, no inventory hedging.
1. No portfolio-level risk model beyond Nautilus defaults (no custom risk engine).
1. No attempt to perfectly match every MakerV3 edge-case or Redis schema field.
1. No distributed orchestration, HA, or production hardening.

## Reference files (source of truth)

Nautilus multi-venue and adapter patterns:
1. [examples/live/bybit/bybit_exec_tester.py](/home/ubuntu/nautilus_trader/examples/live/bybit/bybit_exec_tester.py)
1. [examples/live/binance/binance_data_tester.py](/home/ubuntu/nautilus_trader/examples/live/binance/binance_data_tester.py)
1. [docs/integrations/bybit.md](/home/ubuntu/nautilus_trader/docs/integrations/bybit.md)
1. [docs/integrations/binance.md](/home/ubuntu/nautilus_trader/docs/integrations/binance.md)
1. [docs/concepts/message_bus.md](/home/ubuntu/nautilus_trader/docs/concepts/message_bus.md)

MakerV3 param key conventions (input surface):
1. [chainsaw engine/strategies/maker_v3/task.py](/home/ubuntu/chainsaw/engine/strategies/maker_v3/task.py)

Fluxboard TokenMM UI + FluxAPI backend:
1. [chainsaw fluxboard README.md](/home/ubuntu/chainsaw/fluxboard/README.md)
1. [chainsaw fluxapi blueprint signal.py](/home/ubuntu/chainsaw/fluxapi/blueprints/signal.py)
1. [chainsaw fluxapi realtime strategies.py](/home/ubuntu/chainsaw/fluxapi/realtime/strategies.py)

## Interfaces (contracts)

### Nautilus conventions (implementation guardrails)

Strategy + runner code should follow Nautilus patterns:
1. Use `InstrumentId` (`InstrumentId.from_str(...)`) and `Venue`/adapter constants (`BYBIT`, `BINANCE`) instead of ad-hoc venue strings.
1. Use Nautilus value types (`Price`, `Quantity`) when creating orders (via `order_factory`).
1. Build L2 books via Nautilus `OrderBook` and `OrderBookDeltas` (bookbuilding), not by trusting BBO ticks.
1. Keep external I/O out of the strategy hot-path when possible (use buffered publish + bridge process).

### Redis params (input)

Key format (must match Chainsaw MakerV3):
1. `strategy.<strategy_id>.<param>`

MVP param set:
1. `bot_on` (bool-ish string)
1. `qty` (decimal string)
1. `max_age_ms` (int)
1. Band 1:
1. `bid_edge1` (bps)
1. `ask_edge1` (bps)
1. `n_orders1` (int)
1. `distance1` (bps)
1. `place_edge1` (bps, optional)
1. Band 2:
1. `bid_edge2`, `ask_edge2`, `n_orders2`, `distance2`, `place_edge2`
1. Band 3:
1. `bid_edge3`, `ask_edge3`, `n_orders3`, `distance3`, `place_edge3`

Notes:
1. Treat missing Redis values as "unset" and fall back to defaults in code/config.
1. Treat empty Redis strings as "unset" (matches MakerV3 behavior).

### Redis Streams (from Nautilus MessageBus backing)

Stream key:
1. Set `MessageBusConfig.stream_per_topic=False`.
1. Set `streams_prefix="maker_poc"` and disable trader/id prefixes for a stable stream key.
1. Set `types_filter` to exclude high-volume market data types (otherwise enabling MessageBus backing will flood Redis when subscribed to L2 deltas).

Topics and payloads:
1. `maker_poc.state` payload is a JSON string.
1. `maker_poc.event` payload is a JSON string.
1. `maker_poc.trade` payload is a JSON string.
1. `maker_poc.alert` payload is a JSON string.
1. `maker_poc.balances` payload is a JSON string (bridge uses this to write `balances.snapshot`).
1. `maker_poc.market_bbo` payload is a JSON string (bridge uses this to write `last:*` keys).
1. `maker_poc.fv` payload is a JSON string (bridge uses this to write `fvs.snapshot`).

### Chainsaw Redis outputs (from bridge)

State:
1. `maker_arb:<strategy_id>:state` is JSON.

Events:
1. `maker_arb:<strategy_id>:events` is append-only (list or stream, pick one and be consistent).

Blotters:
1. `trades.blotter` rows on fills.
1. `alerts.blotter` rows on alerts.
1. `balances.snapshot` JSON list and `balances` hash (Fluxboard Balances + Signal readiness).
1. `last:<exchange>:<symbol>` JSON objects (Fluxboard Market Data + Signal helper reads).
1. `fvs.snapshot` JSON list (Fluxboard Signal initial load hard-requires non-empty FV snapshot).

## Minimal message schema (bridge contract)

Bridge and downstream consumers should treat the following keys as stable for the MVP.

State payload (`maker_poc.state` topic, JSON string):
```json
{
  "ts_ms": 0,
  "strategy_id": "STRAT-001",
  "strategy_class": "maker_v3_single_leg",
  "mode": "QUOTING",
  "reason": null,
  "instrument_id": "PLUMEUSDT-LINEAR.BYBIT",
  "venue": "BYBIT",
  "top": {"bid": "0", "ask": "0", "ts_ms": 0},
  "bands": [
    {"band": 1, "bid_edge_bps": "0", "ask_edge_bps": "0", "distance_bps": "0", "n_orders": 0},
    {"band": 2, "bid_edge_bps": "0", "ask_edge_bps": "0", "distance_bps": "0", "n_orders": 0},
    {"band": 3, "bid_edge_bps": "0", "ask_edge_bps": "0", "distance_bps": "0", "n_orders": 0}
  ],
  "orders": {
    "bid": [{"level": 1, "client_order_id": "X", "px": "0", "qty": "0", "status": "WORKING"}],
    "ask": [{"level": 1, "client_order_id": "X", "px": "0", "qty": "0", "status": "WORKING"}]
  }
}
```

Event payload (`maker_poc.event` topic, JSON string):
```json
{
  "ts_ms": 0,
  "strategy_id": "STRAT-001",
  "type": "quote.placed",
  "instrument_id": "ETHUSDT-LINEAR.BYBIT",
  "side": "bid",
  "band": 1,
  "level": 1,
  "px": "0",
  "qty": "0",
  "details": {}
}
```

Trade payload (`maker_poc.trade` topic, JSON string):
```json
{
  "ts_ms": 0,
  "strategy_id": "STRAT-001",
  "type": "fill",
  "instrument_id": "ETHUSDT-LINEAR.BYBIT",
  "side": "buy",
  "px": "0",
  "qty": "0",
  "liquidity": "maker",
  "fee": "0",
  "client_order_id": "X"
}
```

Alert payload (`maker_poc.alert` topic, JSON string):
```json
{
  "ts_ms": 0,
  "strategy_id": "STRAT-001",
  "severity": "warning",
  "message": "stale_market_data",
  "context": {}
}
```

Market BBO payload (`maker_poc.market_bbo` topic, JSON string):
```json
{
  "ts_ms": 0,
  "instrument_id": "PLUMEUSDT-LINEAR.BYBIT",
  "bid": "0",
  "ask": "0"
}
```

FV payload (`maker_poc.fv` topic, JSON string):
```json
{
  "ts_ms": 0,
  "instrument_id": "PLUMEUSDT-LINEAR.BYBIT",
  "fv_bid": "0",
  "fv_ask": "0",
  "update_ts_ms": 0
}
```

Balances payload (`maker_poc.balances` topic, JSON string):
```json
{
  "ts_ms": 0,
  "venue": "BYBIT",
  "rows": [
    {"coin": "USDT", "qty": 0.0, "mark": 1.0, "mv": 0.0, "update_time": "2026-03-03 00:00:00"}
  ]
}
```

## Execution model (parallel agents + worktrees)

We will execute in parallel. Each workstream owns a disjoint file set to avoid merge conflicts.

Required workflow skills:
1. Use `superpowers:using-git-worktrees` before any implementation in a repo.
1. Each agent uses its own worktree and branch.
1. Lead agent integrates by merging branches after per-workstream verification.

Worktree directory convention:
1. Prefer repo-local `.worktrees/` if present and ignored.
1. Otherwise use repo-local `worktrees/` if present and ignored.
1. If neither exists, create `.worktrees/`, add to `.gitignore`, commit, then proceed.

Branch naming:
1. `poc/makerv3-singleleg-scaffold`
1. `poc/makerv3-singleleg-data`
1. `poc/makerv3-singleleg-strategy`
1. `poc/makerv3-singleleg-bridge`
1. `poc/makerv3-singleleg-fluxboard` (in `/home/ubuntu/chainsaw` only; docs/runbook/config tweaks)

Workstream ownership matrix:
1. Scaffold:
1. Owns: `examples/live/poc/README.md`, shared contracts module, runbook docs.
1. Data smoke tests:
1. Owns: `examples/live/poc/multivenue_book_smoke.py`
1. Strategy:
1. Owns: `nautilus_trader/examples/strategies/makerv3_single_leg_quoter.py`, runner script.
1. Bridge:
1. Owns: `examples/live/poc/chainsaw_bridge.py`
1. Fluxboard integration:
1. Owns: changes in `/home/ubuntu/chainsaw` only (runbook, optional config entry), and the validation checklist for TokenMM pages.

## Merge and integration strategy (lead-owned)

Merge order (minimize conflicts):
1. Merge scaffold first.
1. Merge strategy next (it defines the payloads and topics).
1. Merge bridge next (it binds to payload schema).
1. Merge Fluxboard integration notes last (no code changes in Nautilus repo unless needed).
1. Merge data smoke test whenever (independent).

Conflict rule:
1. Only the scaffold branch changes `examples/live/poc/contracts.py` after the first merge.
1. If other branches need contract changes, they submit a follow-up PR/branch to scaffold instead of editing it directly.

## Plan review (what changed vs. earlier version)

1. Removed hedge market, fair value from second leg, and all hedging logic.
1. Refocused strategy on a single `strategy_market` with 3 bands.
1. Kept Binance+Bybit connector setup only as a market data/bookbuilding proof step.
1. Added a workstream-based execution plan with explicit file ownership and merge strategy.

---

## Tasks (bite-sized, parallelizable)

### Task 1: Worktree setup (both repos)

**Owner:** Lead agent.

**Files:**
1. Modify (if needed): `.gitignore`

**Step 1: Create worktree directory**

Run in `/home/ubuntu/nautilus_trader`:
```bash
ls -d .worktrees 2>/dev/null || ls -d worktrees 2>/dev/null || true
```

**Step 2: Verify ignore and create if missing**

If using repo-local `.worktrees/` or `worktrees/`:
```bash
git check-ignore -q .worktrees || git check-ignore -q worktrees
```

If not ignored, add to `.gitignore` and commit.

**Step 3: Create worktrees and branches**

Create worktrees for each workstream:
```bash
git worktree add .worktrees/makerv3-scaffold -b poc/makerv3-singleleg-scaffold
git worktree add .worktrees/makerv3-data -b poc/makerv3-singleleg-data
git worktree add .worktrees/makerv3-strategy -b poc/makerv3-singleleg-strategy
git worktree add .worktrees/makerv3-bridge -b poc/makerv3-singleleg-bridge
```

Create a Chainsaw worktree for Fluxboard integration notes (optional but recommended if you need config/runbook tweaks):
```bash
cd /home/ubuntu/chainsaw
ls -d .worktrees 2>/dev/null || ls -d worktrees 2>/dev/null || true
git check-ignore -q .worktrees || git check-ignore -q worktrees
git worktree add .worktrees/makerv3-fluxboard -b poc/makerv3-singleleg-fluxboard
```

**Step 4: Baseline sanity check**

Run in each worktree (pick one python workflow and stick to it):
```bash
python -V
pytest -q
```

Expected: tests pass or failures are documented as pre-existing.

### Task 2: Scaffold shared POC directory and documentation

**Owner:** Scaffold agent.

**Files:**
1. Create: `examples/live/poc/README.md`
1. Create: `examples/live/poc/__init__.py`
1. Create: `examples/live/poc/contracts.py`

**Step 1: Add POC README**

Include:
1. Env vars for Bybit and Binance (testnet/demo/live).
1. Redis connection env vars for bridge.
1. Run commands for node, bridge, FluxAPI, Fluxboard.

**Step 2: Define contract helpers**

In `contracts.py`, define:
1. `StrategyParams` dataclass or typed dict for the MVP param set.
1. `EventType` string constants for `quote.placed`, `quote.replaced`, `quote.canceled`, `quote.failed`, `fill`, `alert`.
1. Helper `json_dumps_compact(obj) -> str` that only emits JSON primitives.
1. A mapping object for Fluxboard/Redis translation (kept out of the strategy logic):
1. `instrument_id` (Nautilus `InstrumentId` string, e.g. `PLUMEUSDT-LINEAR.BYBIT`)
1. `chainsaw_exchange` (e.g. `bybit_linear`)
1. `chainsaw_symbol` (e.g. `plume/usdt`)
1. `last_key` components are derived from (`chainsaw_exchange`, `chainsaw_symbol`) as `last:<exchange>:<BASE>_<QUOTE>`
1. FV `coin` string uses `chainsaw_symbol` (slash form) so FluxAPI `_find_fv_for_leg` matches.

**Step 3: Commit**

```bash
git add examples/live/poc
git commit -m "poc: add makerv3 single-leg scaffold and contracts"
```

### Task 3: Multi-venue bookbuilding smoke test (Binance + Bybit)

**Owner:** Data agent.

**Files:**
1. Create: `examples/live/poc/multivenue_book_smoke.py`

**Step 1: Implement smoke strategy**

Implement a minimal `Strategy` that:
1. Subscribes to order book deltas and trades for `ETHUSDT-LINEAR.BYBIT`.
1. Subscribes to order book deltas and trades for `ETHUSDT.BINANCE`.
1. Maintains a local `OrderBook` per instrument and logs BBO changes.

**Step 2: Run and verify**

Run:
```bash
python examples/live/poc/multivenue_book_smoke.py
```

Expected:
1. Continuous deltas.
1. BBO updates derived from local book.

**Step 3: Commit**

```bash
git add examples/live/poc/multivenue_book_smoke.py
git commit -m "poc: add multi-venue bookbuilding smoke test"
```

### Task 4: Strategy pure functions (3-band ladder calculation)

**Owner:** Strategy agent.

**Files:**
1. Create: `nautilus_trader/examples/strategies/makerv3_single_leg_quoter.py`
1. Test: `tests/unit_tests/examples/strategies/test_makerv3_single_leg_quoter.py`

**Step 1: Write failing unit tests**

Tests should cover:
1. Given top-of-book bid/ask and params, compute deterministic target prices for each band/level/side.
1. Prices move outward by `distanceX` bps for each additional level.
1. No orders are generated if `n_ordersX` is 0.

**Step 2: Run unit tests (fail)**

Run:
```bash
pytest -q tests/unit_tests/examples/strategies/test_makerv3_single_leg_quoter.py
```

Expected: failing due to missing implementation.

**Step 3: Implement ladder builder**

Implement a pure function like:
```python
def build_band_prices_bps(
    *,
    side: str,
    anchor_px: Decimal,
    edge_bps: Decimal,
    distance_bps: Decimal,
    n_orders: int,
) -> list[Decimal]:
    ...
```

Anchor convention (MVP):
1. Bid anchor is maker best bid.
1. Ask anchor is maker best ask.

Price convention (MVP):
1. Bid price decreases by `(edge_bps + k*distance_bps)` from anchor.
1. Ask price increases by `(edge_bps + k*distance_bps)` from anchor.

**Step 4: Run unit tests (pass)**

Run:
```bash
pytest -q tests/unit_tests/examples/strategies/test_makerv3_single_leg_quoter.py
```

Expected: PASS.

**Step 6: Commit**

```bash
git add nautilus_trader/examples/strategies/makerv3_single_leg_quoter.py
git add tests/unit_tests/examples/strategies/test_makerv3_single_leg_quoter.py
git commit -m "poc: add makerv3 single-leg ladder builder + unit tests"
```

### Task 5: Strategy live order management (place/replace/cancel)

**Owner:** Strategy agent.

**Files:**
1. Modify: `nautilus_trader/examples/strategies/makerv3_single_leg_quoter.py`

**Step 1: Implement Strategy class skeleton**

Implement:
1. `on_start`: subscribe instrument, book deltas, trades.
1. `on_order_book_deltas`: update local book.
1. Throttled on-book handler to run quote cycle (internal constant throttle; no `cooldown` param).

**Step 2: Add slot model**

Implement stable slots per band:
1. Band 1 levels map to 1-10.
1. Band 2 levels map to 11-20.
1. Band 3 levels map to 21-30.

**Step 3: Implement quote cycle**

Inputs:
1. Current maker BBO.
1. Current open orders in slots.
1. Current params.

Outputs:
1. Submit new orders for empty slots.
1. Replace orders whose target price changed beyond a tick threshold or exceeded `max_age_ms`.
1. Cancel orders for slots now out of range (when `n_ordersX` decreases or bot off).

**Step 4: Add minimal event emission hooks**

Emit internal events (in-memory) for:
1. `quote.placed`
1. `quote.replaced`
1. `quote.canceled`
1. `quote.failed`
1. `fill`
1. `alert`

**Step 5: Publish external bridge messages (Nautilus-first schema)**

On each quote cycle (throttled):
1. Publish `maker_poc.state` (JSON string) with current mode + top-of-book + band config + open orders.
1. Publish `maker_poc.market_bbo` (JSON string) for the strategy instrument (this is the bridge's source of truth for `last:*`).
1. Optionally publish `maker_poc.fv` (JSON string) if you want the bridge to be dumb; otherwise bridge derives FV from BBO mid.

On a slower interval (for example 1-5 seconds):
1. Publish `maker_poc.balances` (JSON string) using Nautilus portfolio/account state for the venue.

**Step 5: Commit**

```bash
git add nautilus_trader/examples/strategies/makerv3_single_leg_quoter.py
git commit -m "poc: implement makerv3 single-leg order management"
```

### Task 6: Param ingestion from Chainsaw-style Redis keys

**Owner:** Strategy agent (or Bridge agent if you prefer to centralize Redis access).

**Files:**
1. Modify: `nautilus_trader/examples/strategies/makerv3_single_leg_quoter.py`

**Step 1: Implement param loader**

Implement best-effort polling:
1. Load `strategy.<strategy_id>.<param>` keys.
1. Ignore missing keys.
1. Treat empty strings as unset.
1. Parse types into strongly typed values with safe defaults.

**Step 2: Integrate into quote cycle**

On each cycle:
1. Refresh params if a `params_refresh_interval_ms` has elapsed.
1. If `bot_on` is false:
1. Cancel all managed orders.
1. Emit `mode=OFF` in state.

**Step 3: Commit**

```bash
git add nautilus_trader/examples/strategies/makerv3_single_leg_quoter.py
git commit -m "poc: add redis param polling for makerv3 single-leg"
```

### Task 7: Live node runner (Bybit strategy_market) + MessageBus Redis Streams backing

**Owner:** Strategy agent.

**Files:**
1. Create: `examples/live/poc/makerv3_single_leg_node.py`

**Step 1: Create TradingNodeConfig**

Include:
1. Bybit `data_clients` and `exec_clients` for `BybitProductType.LINEAR`.
1. Instrument provider loads only the selected `strategy_market` instrument id.
1. `message_bus=MessageBusConfig(database=DatabaseConfig(...), encoding="json", timestamps_as_iso8601=True, stream_per_topic=False, streams_prefix="maker_poc", use_trader_prefix=False, use_trader_id=False, use_instance_id=False, types_filter=[QuoteTick, TradeTick, OrderBookDeltas])`

Note: add more types to `types_filter` as needed if you see Redis stream volume explode after enabling MessageBus backing.

**Step 2: Add strategy**

Instantiate strategy with:
1. `strategy_id`
1. `instrument_id`
1. `client_id` pointing at BYBIT exec client id

**Step 3: Run**

Run:
```bash
python examples/live/poc/makerv3_single_leg_node.py
```

Expected:
1. Node connects.
1. Strategy receives market data.
1. Strategy publishes state/events to Redis stream `maker_poc`.

**Step 4: Commit**

```bash
git add examples/live/poc/makerv3_single_leg_node.py
git commit -m "poc: add live node runner with msgbus redis stream backing"
```

### Task 8: Bridge from Redis Streams to Chainsaw keys

**Owner:** Bridge agent.

**Files:**
1. Create: `examples/live/poc/chainsaw_bridge.py`

**Step 1: Implement stream consumer**

Consume stream `maker_poc` using `XREAD` or consumer groups.

**Step 2: Map messages to Chainsaw schema**

Minimum mapping:
1. `maker_poc.state` to `maker_arb:<strategy_id>:state`
1. `maker_poc.event` append to `maker_arb:<strategy_id>:events`
1. `maker_poc.trade` upsert/append to `trades.blotter`
1. `maker_poc.alert` append to `alerts.blotter`
1. `maker_poc.balances` to `balances.snapshot` (plus `balances` hash rows)
1. `maker_poc.market_bbo` to `last:<chainsaw_exchange>:<BASE>_<QUOTE>` using the mapping defined in `contracts.py`
1. `maker_poc.fv` to `fvs.snapshot` using the mapping defined in `contracts.py` (or derive FV mid from `market_bbo` if you want the bridge to be self-contained)

**Step 3: Run**

Run:
```bash
python examples/live/poc/chainsaw_bridge.py
```

Expected:
1. Keys update continuously while node runs.

**Step 4: Commit**

```bash
git add examples/live/poc/chainsaw_bridge.py
git commit -m "poc: add bridge from msgbus stream to chainsaw redis keys"
```

### Task 9: Fluxboard TokenMM UI integration (required GUI)

**Owner:** Fluxboard integration agent.

**Files:**
1. No new Nautilus UI files. This task operates in `/home/ubuntu/chainsaw` to run the existing UI and validate Redis compatibility.

**Step 1: Ensure strategy is visible on TokenMM surface**

Fluxboard TokenMM Signal table filters by strategy meta `strategy_groups=tokenmm` from `configs/strategies.ini`:
1. Prefer reusing an existing TokenMM MakerV3 strategy id already defined in Chainsaw `configs/strategies.ini` (example: `bybit_binance_plumeusdt_makerv3`) to avoid config changes.
1. If you create a new strategy id, add a new `[strategy:<id>]` section with:
1. `class = maker_v3`
1. `strategy_groups = tokenmm`
1. `exchange = bybit_linear` and a valid market key (plus `hedge_exchange` if FluxAPI expects it for builder paths).

**Step 2: Start FluxAPI backend**

Run in `/home/ubuntu/chainsaw`:
```bash
gunicorn -k eventlet -w 1 -b 0.0.0.0:${PORT:-5000} fluxapi.web:app
```

**Step 3: Start Fluxboard**

Production mode (served by Flask) uses the built assets already wired in Chainsaw.

If you need dev mode (optional), in `/home/ubuntu/chainsaw/fluxboard`:
```bash
pnpm install
pnpm dev
```

**Step 4: Validate TokenMM panels**

Navigate to:
1. `/tokenmm/signal`
1. `/tokenmm/params`
1. `/tokenmm/balances`
1. `/tokenmm/trades`
1. `/tokenmm/alerts`

Expected:
1. Strategy row exists and updates (REST initial load; WebSocket updates if enabled).
1. Params read/write round-trips to `strategy.<id>.*` keys.
1. Balances show at least Bybit rows.
1. Trades populate on fills.
1. Alerts populate on errors/blocked states.

### Task 10: End-to-end runbook + acceptance criteria

**Owner:** Scaffold agent (with input from all).

**Files:**
1. Modify: `examples/live/poc/README.md`

**Step 1: Document run order**

Document:
1. Start Redis.
1. Start Nautilus node runner.
1. Start bridge.
1. Start FluxAPI + Fluxboard (TokenMM UI).

**Step 2: Acceptance checklist**

Checklist:
1. Bybit bookbuilding works and quoting places 3-band ladders.
1. `bot_on=false` cancels all quotes within one cycle.
1. Redis keys update: state, events, alerts (and trades when fills happen).
1. Fluxboard TokenMM pages (Signal/Params/Balances/Trades/Alerts) show the strategy and update.

**Step 3: Commit**

```bash
git add examples/live/poc/README.md
git commit -m "docs: add makerv3 single-leg poc runbook and acceptance checklist"
```

---

## Execution decisions (locked 2026-03-03)

1. `strategy_id` for Fluxboard TokenMM target is **Option A** (reuse existing config): `bybit_binance_plumeusdt_makerv3`.
1. Strategy execution venue is Bybit linear perp only (single execution leg).
1. Market data venues are Bybit + Binance (Binance data-only) for bookbuilding validation and Fluxboard leg hydration.
1. Integration order is fixed: scaffold -> strategy -> bridge -> data (chainsaw notes last).

## Open questions (resolve early in execution)

1. `place_edgeX` semantics:
1. MVP can treat it as extra bps away from anchor.
1. If Flux UI expects a specific meaning, reconcile later.
1. Events list storage type:
1. Use a Redis list (`RPUSH`) for simplicity, or a stream if you need per-event IDs.
1. Fluxboard data prerequisites:
1. Signal page hard-requires non-empty `fvs.snapshot`. Decide whether bridge publishes a minimal FV snapshot, or you run an existing FV publisher.

## Improvements backlog (post-MVP)

1. Replace Redis param polling with a typed config feed and evented updates.
1. Add a deterministic "dry-run" mode that replays recorded book deltas to test order logic without live trading.
1. Add a consumer-group based bridge for at-least-once delivery and crash recovery.
1. Add explicit Nautilus RiskEngine config for max orders, max position, notional limits.

## Execution tracking (lead-owned)

Update this table during orchestration:

| Workstream | Branch | Worktree path | Owner | Status | Notes |
|---|---|---|---|---|---|
| Scaffold | `poc/makerv3-singleleg-scaffold` | `.worktrees/makerv3-scaffold` | `Heisenberg` | `done` | `commit 3d7e7580; review APPROVED` |
| Data | `poc/makerv3-singleleg-data` | `.worktrees/makerv3-data` | `Banach` | `done` | `commit cd3b7980; review APPROVED` |
| Strategy | `poc/makerv3-singleleg-strategy` | `.worktrees/makerv3-strategy` | `Franklin` | `done` | `commit d3acc35a; review APPROVED` |
| Bridge | `poc/makerv3-singleleg-bridge` | `.worktrees/makerv3-bridge` | `Boyle -> Galileo` | `done` | `commits c6a35b3d + a0af7cda; review+fix loop complete` |
| Fluxboard integration | `poc/makerv3-singleleg-fluxboard` | `/home/ubuntu/chainsaw/.worktrees/makerv3-fluxboard` | `Darwin -> Fermat -> Boole` | `done` | `commits db671dcf + 4dc394ce + f5eb2257; docs lint green` |
