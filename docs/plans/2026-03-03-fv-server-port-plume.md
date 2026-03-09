# Chainsaw FV Server (PLUME) Port Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Port Chainsaw's Redis-driven FV pricing engine into `nautilus_trader` so FV is produced **outside strategies** and can be consumed via Nautilus' **message bus + cache**, with a working **PLUME (PLUME/USDT)** configuration.

**Architecture:** Port the FV math/config from Chainsaw (`engine/fvserver/*`) into a Nautilus-native `Actor` (`FvServerActor`) which subscribes to the required market data (spot order book depth for P2F + perp mid + perp trades), computes FV on a timer, and publishes a richer `CustomData` snapshot for strategy/UI/debug. Publishing `MarkPriceUpdate` is supported but **opt-in** (`publish_mark_price=False` by default) to avoid accidentally overriding venue-provided mark semantics in live trading. Keep an optional Redis-compat bridge as a follow-up if other systems still expect Chainsaw's Redis topics.

**Tech Stack:** Python (Nautilus `Actor`, msgspec config, pytest) + existing Nautilus Binance live adapters + Nautilus cache/message bus. Optional: `redis` (redis-py) for compatibility mode.

---

## Status Tracker

- [ ] Task 1: FV models + payload contract
- [ ] Task 2: FV config + safe publish toggles
- [ ] Task 3: FV utils (time units + side normalization)
- [ ] Task 4: P2F computation + insufficient-depth semantics
- [ ] Task 5: FV engine port + health/staleness
- [ ] Task 6: Golden/parity tests vs Chainsaw
- [ ] Task 7: `FvServerActor` wiring + publishing
- [ ] Task 8: PLUME live example (Binance spot + futures)
- [ ] Task 9 (Optional): Redis compatibility bridge
- [ ] Task 10: Docs + verification gates

## Acceptance Criteria (MVP)

- FV publishes `CustomData` snapshots at `tick_interval_ms` with stable schema + versioning (`fv_profile`, `fv_version`).
- P2F returns `None` when book depth is insufficient and health marks data degraded/stale.
- `MarkPriceUpdate` publishing is **disabled by default** and only enabled intentionally.
- A fixed input sequence produces snapshots matching Chainsaw within tolerances (golden test).

## Research Summary (Chainsaw "source of truth")

### FV Server implementation (Chainsaw)

Core pricing service:
- `/home/ubuntu/chainsaw/engine/fvserver/main.py` (entrypoint: `python3 -m engine.fvserver.main --config <ini>`)
- `/home/ubuntu/chainsaw/engine/fvserver/config.py` (INI parsing, default terms/config knobs)
- `/home/ubuntu/chainsaw/engine/fvserver/engine.py` (FV state + term math)
- `/home/ubuntu/chainsaw/engine/fvserver/publisher.py` (Redis publish/write semantics)
- `/home/ubuntu/chainsaw/engine/fvserver/types.py` (typed config/data structures)

PLUME config file:
- `/home/ubuntu/chainsaw/configs/fv/fvserver_plume_v1.ini`
  - `symbol = PLUME_USDT`
  - `p2f_channel = md:p2f:binance_spot:PLUME_USDT`
  - `perp_mid_channel = md:binance_perp:PLUME_USDT`
  - `trade_channel = md:trades:binance_perp:PLUME_USDT`
  - bootstrap keys:
    - `p2f_last_key = last:p2f:binance_spot:PLUME_USDT`
    - `perp_mid_last_key = last:binance_perp:PLUME_USDT`

FV does **not** load ML model artifacts. PLUME in FV context is just the **symbol/profile** configuration.

### Chainsaw transport contract (Redis)

Inbound (FV server subscribes):
- `md:p2f:binance_spot:<SYMBOL>`
  - must include at least: `symbol`, `ts_ms`, `p2f_mid_50k`, `p2f_mid_100k`
- `md:binance_perp:<SYMBOL>`
  - must include a mid or bid/ask (varies by publisher)
- `md:trades:binance_perp:<SYMBOL>`
  - must include at least: `price`, `qty`, `side` (`buy|sell`), optional `ts_ms`

Bootstrap reads:
- `last:p2f:binance_spot:<SYMBOL>`
- `last:binance_perp:<SYMBOL>`

Outbound (FV server publishes + stores "last"):
- `fv:last:{profile}:{SYMBOL}` (plus legacy alias `fv:last:{SYMBOL}` for `profile=fv1`)
- `fv:update:{profile}:{SYMBOL}` (plus legacy alias)
- `fv:health:{profile}:{SYMBOL}` (plus legacy alias)

Downstream API (Chainsaw FluxAPI) exists but is not required for Nautilus MVP:
- `/home/ubuntu/chainsaw/fluxapi/blueprints/fv.py` (`GET /api/v1/fv/<symbol>/latest`, etc.)

### Chainsaw P2F computation (upstream of FV)

P2F (price-to-fill) is computed in Chainsaw's market data pipeline:
- `/home/ubuntu/chainsaw/engine/market_data/binance_spot/p2f.py`
- `/home/ubuntu/chainsaw/engine/market_data/binance_spot/ws_publisher.py`

For Nautilus integration, either:
1. Port this P2F logic and compute P2F directly from Nautilus-managed spot order books, or
2. Keep a dedicated P2F producer (outside strategies) and feed the FV engine with the same semantics.

---

## Proposed Nautilus Design (recommended)

### Transport: use Nautilus msgbus + cache (no Redis in MVP)

Nautilus already provides:
- message bus pub/sub and topic matching: `/home/ubuntu/nautilus_trader/nautilus_trader/common/component.pyx`
- actor subscriptions and helpers: `/home/ubuntu/nautilus_trader/nautilus_trader/common/actor.pyx`
- managed order books in cache: `/home/ubuntu/nautilus_trader/nautilus_trader/data/engine.pyx` creates `OrderBook` and stores it in cache (`cache.order_book(...)`)
- `CustomData` wrapper for rich custom payloads: `/home/ubuntu/nautilus_trader/nautilus_trader/model/data.pyx` (`DataType`, `CustomData`)
- `MarkPriceUpdate` support to feed portfolio/risk: `MarkPriceUpdate` appears in `nautilus_trader.model.data`

So the cleanest design is:
- `FvServerActor` subscribes to spot order book deltas/depth, perp quote ticks, perp trade ticks.
- Actor computes:
  - P2F mids at $50k and $100k notional from the *current spot book*,
  - perp mid from quote ticks,
  - signed volume from trades with exponential decay,
  - FV terms via ported Chainsaw FV engine.
- Actor publishes:
  - `CustomData` snapshot with the full FV payload (terms, what_moved, health) for other consumers.
  - optional: `MarkPriceUpdate` for `publish_instrument_id` using the computed `final` FV as the mark (guarded by `publish_mark_price=True`).
- Actor stores latest snapshot:
  - always keep an in-memory `_last_snapshot` for internal reads and debugging.
  - optional: persist the latest snapshot as `bytes` via `cache.add(key: str, value: bytes)` (note: the generic cache API stores `bytes`, not Python objects).

### Compatibility mode (optional follow-up)

If something external still expects Chainsaw Redis topics, add an opt-in `RedisFvBridge`:
- publish the same `fv:last:*` and `fv:update:*` semantics to Redis
- optionally subscribe to `md:*` Redis topics and republish into Nautilus for drop-in replacement.

This is intentionally NOT MVP because it introduces a dependency and splits truth across two buses.

---

## PLUME (v1) Configuration Mapping

Chainsaw uses `SYMBOL=PLUME_USDT` (underscore). Nautilus' Binance examples use symbols like:
- spot: `PLUMEUSDT.BINANCE_SPOT`
- perp: `PLUMEUSDT-PERP.BINANCE_FUTURES`

For this port, define a single "pricing symbol" concept which maps to:
- `spot_instrument_id` (for P2F order book)
- `perp_instrument_id` (for mid + trades)
- `publish_instrument_id` (where FV should publish; usually the perp, and `MarkPriceUpdate` only if explicitly enabled)

Config knobs to carry over (from Chainsaw INI):
- `tick_interval_ms`
- `signed_volume_half_life_ms`
- `overlay_max_pct`
- `fv_profile` (default `fv1`)
- `fv_version` (string; used for downstream compatibility and cache keys)
- `calc_type`, `comp_theo_mode` (preserve as-is; don’t reinterpret in MVP)
- `stale_threshold_ms` / `publish_health_every_ms` (health cadence)

---

## Implementation Plan (TDD, bite-sized steps)

### Task 1: Create FV data models for Nautilus publishing

**Files:**
- Create: `nautilus_trader/live/fvserver/__init__.py`
- Create: `nautilus_trader/live/fvserver/models.py`
- Test: `tests/unit_tests/live/test_fvserver_models.py`

**Step 1: Write failing tests for `FairValueSnapshot` contract**

Create `tests/unit_tests/live/test_fvserver_models.py`:
```python
from nautilus_trader.model.data import DataType, CustomData
from nautilus_trader.live.fvserver.models import FairValueSnapshot


def test_fv_snapshot_can_be_wrapped_as_custom_data():
    snap = FairValueSnapshot(
        ts_event=1,
        ts_init=2,
        symbol="X",
        fv_profile="fv1",
        fv_version="v1",
        calc_type="fv",
        final=123.0,
        base=123.0,
        overlay_pct=0.0,
        signed_volume=0.0,
        terms=[],
        what_moved=[],
    )
    dt = DataType(FairValueSnapshot, metadata={"instrument_id": "X"})
    wrapped = CustomData(data_type=dt, data=snap)
    assert wrapped.ts_event == 1
    assert wrapped.ts_init == 2
```

**Step 2: Run test to verify it fails**

Run: `make pytest`
Expected: FAIL (missing module/class).

**Step 3: Add `FairValueSnapshot` + `FairValueHealth` models**

Create `nautilus_trader/live/fvserver/models.py`:
- Use plain Python classes or `@dataclass(frozen=True)` with:
  - `ts_event: int` (ns)
  - `ts_init: int` (ns)
  - `symbol: str`
  - `fv_profile: str`
  - `fv_version: str`
  - `calc_type: str`
  - `final: float`
  - `base: float`
  - `overlay_pct: float`
  - `signed_volume: float`
  - `terms: list[dict]` (or a typed term model if easy)
  - `what_moved: list[str]` (or list of dicts matching Chainsaw)

**Step 4: Run test to verify it passes**

Run: `make pytest`
Expected: PASS for the new unit test.

**Step 5: Commit**

```bash
git add nautilus_trader/live/fvserver/__init__.py nautilus_trader/live/fvserver/models.py tests/unit_tests/live/test_fvserver_models.py
git commit -m "feat(fv): add FV snapshot models"
```

---

### Task 2: Port FV config (Chainsaw INI -> Nautilus msgspec config)

**Files:**
- Create: `nautilus_trader/live/fvserver/config.py`
- Test: `tests/unit_tests/live/test_fvserver_config.py`
- Reference (do not edit): `/home/ubuntu/chainsaw/configs/fv/fvserver_plume_v1.ini`
- Reference (do not edit): `/home/ubuntu/chainsaw/engine/fvserver/config.py`

**Step 1: Write failing test for parsing a PLUME-like config**

Create `tests/unit_tests/live/test_fvserver_config.py`:
```python
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.live.fvserver.config import FvServerActorConfig


def test_fv_config_constructs_plume_defaults():
    cfg = FvServerActorConfig(
        pricing_symbol="PLUME_USDT",
        spot_instrument_id=InstrumentId.from_str("PLUMEUSDT.BINANCE_SPOT"),
        perp_instrument_id=InstrumentId.from_str("PLUMEUSDT-PERP.BINANCE_FUTURES"),
        publish_instrument_id=InstrumentId.from_str("PLUMEUSDT-PERP.BINANCE_FUTURES"),
    )
    assert cfg.fv_profile == "fv1"
    assert cfg.overlay_max_pct > 0.0
```

**Step 2: Run test to verify it fails**

Run: `make pytest`
Expected: FAIL (missing module/class).

**Step 3: Implement `FvServerActorConfig`**

Create `nautilus_trader/live/fvserver/config.py`:
- Use `msgspec.Struct` (frozen) and follow existing config conventions (see `nautilus_trader/live/config.py`).
- Include fields:
  - `pricing_symbol: str` (e.g. `PLUME_USDT` for compatibility tags only)
  - `spot_instrument_id: InstrumentId`
  - `perp_instrument_id: InstrumentId`
  - `publish_instrument_id: InstrumentId`
  - `publish_custom_data: bool = True`
  - `publish_mark_price: bool = False` (default off to avoid overriding venue marks)
  - `p2f_notional_1: float = 50_000.0`
  - `p2f_notional_2: float = 100_000.0`
  - `tick_interval_ms: int = 200` (match Chainsaw defaults if present)
  - `signed_volume_half_life_ms: int = 2_000` (match Chainsaw defaults if present)
  - `overlay_max_pct: float = 0.02` (match Chainsaw)
  - `fv_profile: str = "fv1"`
  - `fv_version: str = "v1"`
  - `calc_type: str = "fv"`
  - `comp_theo_mode: str = "default"`
  - health knobs (`publish_health_every_ms`, `stale_threshold_ms`)
- Add validation where necessary (non-negative intervals, etc.) consistent with other configs.

**Step 4: Run test to verify it passes**

Run: `make pytest`
Expected: PASS.

**Step 5: Commit**

```bash
git add nautilus_trader/live/fvserver/config.py tests/unit_tests/live/test_fvserver_config.py
git commit -m "feat(fv): add FV server actor config"
```

---

### Task 3: Add FV utility helpers (time units + trade side normalization)

**Files:**
- Create: `nautilus_trader/live/fvserver/utils.py`
- Test: `tests/unit_tests/live/test_fvserver_utils.py`

**Step 1: Write failing tests for time conversions and side normalization**

Create `tests/unit_tests/live/test_fvserver_utils.py`:
```python
from nautilus_trader.live.fvserver.utils import ms_to_ns, ns_to_ms, normalize_trade_side


def test_ms_to_ns_roundtrip():
    assert ns_to_ms(ms_to_ns(123)) == 123


def test_normalize_trade_side_unknown():
    assert normalize_trade_side(None) == "unknown"
```

**Step 2: Run test to verify it fails**

Run: `make pytest`
Expected: FAIL (missing module/functions).

**Step 3: Implement minimal helpers**

In `nautilus_trader/live/fvserver/utils.py`:
- `ms_to_ns(ms: int) -> int`
- `ns_to_ms(ns: int) -> int`
- `normalize_trade_side(side: object) -> str` returning `"buy"|"sell"|"unknown"`

**Step 4: Run test to verify it passes**

Run: `make pytest`
Expected: PASS.

**Step 5: Commit**

```bash
git add nautilus_trader/live/fvserver/utils.py tests/unit_tests/live/test_fvserver_utils.py
git commit -m "feat(fv): add fvserver utils"
```

---

### Task 4: Port P2F calculation (spot book -> p2f_mid_50k/p2f_mid_100k)

**Files:**
- Create: `nautilus_trader/live/fvserver/p2f.py`
- Test: `tests/unit_tests/live/test_fvserver_p2f.py`
- Reference (do not edit): `/home/ubuntu/chainsaw/engine/market_data/binance_spot/p2f.py`

**Step 1: Write failing unit tests for P2F**

Create `tests/unit_tests/live/test_fvserver_p2f.py`:
```python
from nautilus_trader.live.fvserver.p2f import p2f_from_l2


def test_p2f_mid_simple_book():
    # bids/asks are (price, qty_base)
    bids = [(1.00, 1000.0), (0.99, 2000.0)]
    asks = [(1.01, 1000.0), (1.02, 2000.0)]
    res = p2f_from_l2(bids=bids, asks=asks, notional_quote=1000.0)
    assert res.mid is not None
    assert res.mid > 1.00 and res.mid < 1.02


def test_p2f_insufficient_depth_returns_none():
    bids = [(1.00, 1.0)]
    asks = [(1.01, 1.0)]
    res = p2f_from_l2(bids=bids, asks=asks, notional_quote=1_000_000.0)
    assert res.mid is None
```

**Step 2: Run test to verify it fails**

Run: `make pytest`
Expected: FAIL (missing function).

**Step 3: Implement P2F helper(s)**

In `nautilus_trader/live/fvserver/p2f.py`:
- Implement a pure function which returns a rich result (for health/debug), e.g.:
  - `P2FResult(mid: float | None, buy_vwap: float | None, sell_vwap: float | None, filled_buy: bool, filled_sell: bool)`
- The function should:
  - walks asks to simulate a buy fill until `notional_quote` is reached
  - walks bids to simulate a sell fill until `notional_quote` is reached
- Mid = `(buy_vwap + sell_vwap) / 2` only when both sides fully fill; otherwise `mid=None`.
- Be explicit about units: qty is base, notional is quote.
- Insufficient depth behavior (recommended): return `mid=None` and let FV health degrade/stale until inputs recover.

**Step 4: Run test to verify it passes**

Run: `make pytest`
Expected: PASS.

**Step 5: Commit**

```bash
git add nautilus_trader/live/fvserver/p2f.py tests/unit_tests/live/test_fvserver_p2f.py
git commit -m "feat(fv): add P2F calculation helpers"
```

---

### Task 5: Port Chainsaw FV engine math (core state + snapshot payload)

**Files:**
- Create: `nautilus_trader/live/fvserver/engine.py`
- Test: `tests/unit_tests/live/test_fvserver_engine.py`
- Reference (do not edit): `/home/ubuntu/chainsaw/engine/fvserver/engine.py`
- Reference (do not edit): `/home/ubuntu/chainsaw/engine/fvserver/types.py`

**Step 1: Write failing test that asserts snapshot fields exist**

Create `tests/unit_tests/live/test_fvserver_engine.py`:
```python
from nautilus_trader.live.fvserver.engine import FvEngine


def test_fv_engine_produces_snapshot_dict():
    eng = FvEngine(
        symbol="PLUME_USDT",
        fv_profile="fv1",
        fv_version="v1",
        overlay_max_pct=0.02,
        signed_volume_half_life_ms=2000,
    )
    eng.on_p2f(ts_ms=1, p2f_mid_50k=1.0, p2f_mid_100k=1.1)
    eng.on_perp_mid(ts_ms=2, mid=1.05)
    eng.on_trade(ts_ms=3, price=1.05, qty=10.0, side="buy")
    snap = eng.snapshot(ts_ms=4, trigger="timer")
    assert snap["symbol"] == "PLUME_USDT"
    assert "final" in snap
    assert "terms" in snap
```

**Step 2: Run test to verify it fails**

Run: `make pytest`
Expected: FAIL (missing engine).

**Step 3: Implement `FvEngine` by porting Chainsaw**

In `nautilus_trader/live/fvserver/engine.py`:
- Port the minimal subset of Chainsaw's FV logic required to:
  - track latest p2f, perp mid, trades
  - compute signed volume with decay
  - compute base + overlay + final
  - build a snapshot dict compatible with Chainsaw fields (at least those listed in Research Summary)
- Keep timestamps in ms internally for easier porting, then convert to ns at publish boundary.
- Preserve field names (`fv_profile`, `fv_version`, `calc_type`, `what_moved`, etc.) so any future Redis-compat or UI can reuse.
- Add explicit health/staleness in the engine:
  - track `last_ts_ms` per input and compute stale flags in `snapshot(ts_ms=...)`
  - clamp overlay to `[-overlay_max_pct, +overlay_max_pct]`
- Trade side handling:
  - accept `"buy"|"sell"|"unknown"` and ignore or neutralize `"unknown"` in signed-volume updates (track count in health).

**Step 4: Run test to verify it passes**

Run: `make pytest`
Expected: PASS.

**Step 5: Commit**

```bash
git add nautilus_trader/live/fvserver/engine.py tests/unit_tests/live/test_fvserver_engine.py
git commit -m "feat(fv): port Chainsaw FV engine core"
```

---

### Task 6: Add golden/parity fixtures vs Chainsaw

**Files:**
- Create: `tests/test_data/fvserver/chainsaw_plume_fv1_golden.json`
- Create: `tests/unit_tests/live/test_fvserver_parity.py`
- (Optional) Create: `scripts/fvserver/generate_chainsaw_golden.py`
- Reference (do not edit): `/home/ubuntu/chainsaw/engine/fvserver/engine.py`

**Step 1: Capture a small deterministic input sequence**

Create a short sequence of inputs (p2f, perp mid, trades) for `PLUME_USDT` with fixed timestamps and store it in the golden JSON alongside expected snapshots.

**Step 2: Generate expected snapshots using Chainsaw engine**

If Chainsaw is available locally, add an optional generator script to produce expected snapshot JSON from `/home/ubuntu/chainsaw/engine/fvserver/engine.py`.

**Step 3: Write parity test**

In `tests/unit_tests/live/test_fvserver_parity.py`:
- load the golden file
- replay inputs into the Nautilus `FvEngine`
- assert key outputs match within tolerances (`final`, `base`, `overlay_pct`, and any stable term fields)

**Step 4: Run tests**

Run: `make pytest`
Expected: PASS.

**Step 5: Commit**

```bash
git add tests/test_data/fvserver/chainsaw_plume_fv1_golden.json tests/unit_tests/live/test_fvserver_parity.py scripts/fvserver/generate_chainsaw_golden.py
git commit -m "test(fv): add chainsaw parity fixtures"
```

---

### Task 7: Implement `FvServerActor` (subscribe -> compute -> publish CustomData, optional MarkPriceUpdate)

**Files:**
- Create: `nautilus_trader/live/fvserver/actor.py`
- Modify: `nautilus_trader/live/fvserver/__init__.py`
- Test: `tests/unit_tests/live/test_fvserver_actor.py`
- Reference (do not edit): `/home/ubuntu/nautilus_trader/nautilus_trader/common/actor.pyx`
- Reference (do not edit): `/home/ubuntu/nautilus_trader/nautilus_trader/common/data_topics.pyx`

**Step 1: Write failing actor test (smoke)**

Create `tests/unit_tests/live/test_fvserver_actor.py` with a minimal harness:
- instantiate actor with config
- call its lifecycle hooks (`on_start`) if available in the test harness
- simulate receiving:
  - order book deltas or a cached order book state
  - quote tick for perp mid
  - trade tick for perp signed volume
- assert it publishes at least:
  - a `CustomData` snapshot (required)
  - and only publishes `MarkPriceUpdate` when `publish_mark_price=True`

If a full msgbus harness is heavy, start with a unit-level test of the actor method which builds the outbound objects.

**Step 2: Run test to verify it fails**

Run: `make pytest`
Expected: FAIL.

**Step 3: Implement `FvServerActor`**

In `nautilus_trader/live/fvserver/actor.py`:
- Subclass `nautilus_trader.common.actor.Actor`.
- In `on_start`:
  - call `subscribe_order_book_deltas(config.spot_instrument_id, book_type=..., managed=True)`
  - call `subscribe_quote_ticks(config.perp_instrument_id, ...)` (or book ticker stream if preferred)
  - call `subscribe_trade_ticks(config.perp_instrument_id, ...)`
  - start a repeating timer every `tick_interval_ms` to publish snapshots.
- For P2F:
  - read `spot_book = self.cache.order_book(config.spot_instrument_id)` and compute p2f mids using `p2f_mid_from_l2(...)`.
- For perp mid/trades:
  - update the engine on `on_quote_tick` / `on_trade_tick` handlers.
- On each publish tick:
  - build snapshot dict via `FvEngine.snapshot(...)`
  - emit:
    - always: `CustomData(DataType(FairValueSnapshot, metadata={"instrument_id": publish_instrument_id, "fv_profile": ..., "fv_version": ...}), FairValueSnapshot(...))`
    - optional: `MarkPriceUpdate` (for `publish_instrument_id`) with price = snapshot["final"] when `publish_mark_price=True`
  - store the latest snapshot:
    - in-memory `_last_snapshot` (always)
    - optional serialized bytes in cache under a deterministic key, e.g.:
      - `cache.add(f"fv:{publish_instrument_id}:{fv_profile}:{fv_version}", msgspec.json.encode(snapshot_dict))`

**Step 4: Run tests**

Run: `make pytest`
Expected: PASS.

**Step 5: Commit**

```bash
git add nautilus_trader/live/fvserver/__init__.py nautilus_trader/live/fvserver/actor.py tests/unit_tests/live/test_fvserver_actor.py
git commit -m "feat(fv): add FV server actor"
```

---

### Task 8: Add a PLUME live example (Binance spot + futures)

**Files:**
- Create: `examples/live/binance/binance_plume_fvserver.py`
- Modify (optional): `examples/live/binance/README.md` (if present; otherwise keep in script docstring)

**Step 1: Add example script skeleton**

Follow the pattern in:
- `examples/live/binance/binance_spot_and_futures_market_maker.py`

The example should:
1. Create a `TradingNodeConfig` with:
   - `BINANCE_SPOT` data client (spot)
   - `BINANCE_FUTURES` data client (USDT futures)
2. Build a `TradingNode`.
3. Add the FV actor:
   - `node.trader.add_actor(FvServerActor(config=FvServerActorConfig(...)))`
4. Subscribe the actor’s required data streams via its `on_start`.
5. Run node.

**Step 2: Run example (manual smoke)**

Run (example):
```bash
uv run --active --no-sync python examples/live/binance/binance_plume_fvserver.py
```
Expected:
- logs show subscriptions active
- periodic FV publish events occur
- mark prices update for the publish instrument only when `publish_mark_price=True`

**Step 3: Commit**

```bash
git add examples/live/binance/binance_plume_fvserver.py
git commit -m "examples(fv): add Binance PLUME fvserver example"
```

---

### Task 9 (Optional): Redis compatibility bridge for Chainsaw topic contract

**Files:**
- Create: `nautilus_trader/live/fvserver/redis_bridge.py`
- Modify: `pyproject.toml` (add redis dependency)
- Test: `tests/unit_tests/live/test_fvserver_redis_bridge.py`

**Step 1: Decide direction**

Pick one:
1. `Nautilus -> Redis` only (publish `fv:last:*` + `fv:update:*` for external consumers)
2. `Redis -> Nautilus` only (consume `md:*` topics to feed internal actor)
3. Bi-directional (more complex; avoid unless required)

Recommended: start with `Nautilus -> Redis` only.

**Step 2: Add dependency**

Add `redis` (and optionally `hiredis`) to `pyproject.toml` under the appropriate dependency group.

**Step 3: Implement bridge**

- Use Redis pubsub pattern subscriptions for `md:*` if needed.
- Publish output keys and channels matching Chainsaw semantics:
  - `fv:last:{profile}:{symbol}` and legacy aliases for `fv1`
  - `fv:update:{profile}:{symbol}`
  - `fv:health:{profile}:{symbol}`

**Step 4: Tests**

If there is no Redis test harness in repo, keep this as an integration test behind docker services, or provide a fake Redis client and unit test encoding/key naming only.

---

### Task 10: Docs + verification gates

**Files:**
- Create: `docs/integrations/fvserver.md`
- Modify: `docs/integrations/README.md` (if it exists and has an index)

**Step 1: Document the FV actor**

Include:
- required data subscriptions
- symbol mapping (PLUME_USDT vs InstrumentId)
- how to consume:
  - custom data (topic + `CustomData` type) (recommended default)
  - mark prices (`cache.mark_price(instrument_id)`) only if `publish_mark_price=True`
  - optional: latest snapshot bytes via `cache.get("fv:<instrument_id>:<profile>:<version>")` if enabled
- staleness/health semantics + recommended strategy behavior

**Step 2: Verification checklist**

Run:
- `make pytest`
- (optional) `make cargo-test-core` if any Rust/Cython touched
- live smoke example script for PLUME

---

## Open Questions (answer before heavy implementation)

1. Do you need *drop-in compatibility* with Chainsaw Redis topics (`fv:last:*`, etc.), or is Nautilus msgbus+cache the new source of truth?
   - Recommended default: Nautilus msgbus+cache only (no Redis in MVP).
2. Should FV publish as `MarkPriceUpdate` for portfolio integration, or strictly as `CustomData` (leaving strategy to decide how to use it)?
   - Recommended default: always publish `CustomData`; gate `MarkPriceUpdate` behind `publish_mark_price=False` by default.
3. Is PLUME FV for spot, perp, or both? (This determines `publish_instrument_id` and how portfolio should interpret the mark.)
   - Recommended default: compute from spot+perp inputs and publish against the perp instrument.
