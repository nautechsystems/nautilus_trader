# Bitget Exchange Integration Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a first-class Bitget adapter to NautilusTrader which supports **live market data ingest + order execution** for **Spot**, **USDT-FUTURES**, **COIN-FUTURES**, and **USDC-FUTURES** (perps + delivery futures where applicable), including Bitget **Demo (paper trading)** support.

**Architecture:** Add a new Rust adapter crate (`nautilus-bitget`) providing Bitget **HTTP** and **WebSocket** clients + data/execution factories and **PyO3 bindings** (aggregated into `nautilus_trader._libnautilus` / `nautilus_trader.core.nautilus_pyo3`). Add a matching Python adapter package (`nautilus_trader.adapters.bitget`) which follows the existing OKX/Bybit patterns (instrument provider + live data client + live execution client + factories), using the Rust Bitget clients for transport + parsing, and emitting normalized Nautilus `Data` and execution reports/events.

**Tech Stack:** Rust (tokio, tokio-tungstenite, serde, base64/HMAC, pyo3) + Python (asyncio, msgspec configs, Nautilus live client base classes) + Bitget **Classic v2** REST/WS APIs.

---

## API version choice (lock this early)

Bitget has multiple API “account types” and versions:
1. **Classic (v2)**: REST `.../api/v2/...` and WebSocket `wss://ws.bitget.com/v2/ws/...`.
1. **UTA (v3)**: REST `.../api/v3/...` and WebSocket `wss://ws.bitget.com/v3/ws/...`.

This plan targets **Classic (v2)** for both Spot and Futures. If you want to target UTA instead, write a separate plan (request/response models and WS channels differ).

## Scope (MVP)

MVP scope is locked to:
1. **Spot + USDT-FUTURES + COIN-FUTURES + USDC-FUTURES**.

### Venues/products
1. **Spot** (Classic v2).
1. **USDT-FUTURES** (perpetual + delivery futures; Classic v2).

### Market data
1. Instruments/instrument updates.
1. L2 order book deltas (depth) via WebSocket, with REST snapshot fallback on gaps.
1. Trade ticks via WebSocket.
1. Bars via REST requests and live WS candle streams.
1. Funding rates + mark/index/symbol prices for futures (REST + WS ticker channels).

### Execution
1. Place/cancel/modify orders via REST (do *not* depend on WS “place-order” channels which may require special permissions).
1. Reconcile order status + fills via **private WebSocket** channels with REST fallback for missed updates.
1. Account state: spot assets; futures account + positions.
1. `cancel_all_orders()` supported via safe “cancel owned orders” logic:
   - default: query open orders and cancel individually/batch by ID.
   - only use Bitget “cancel all” endpoints when it’s safe (single-strategy / explicit opt-in), because they are product-wide.

### Environments
1. Mainnet.
1. Demo / paper trading (Classic): add header `paptrading: 1` for REST, and use `wss://wspap.bitget.com/v2/ws/*` for WS.

## Explicit non-goals (MVP)
1. UTA v3 API support (treat as a follow-up phase once Classic is stable).
1. Historical quote-tick requests remain unsupported because Bitget does not publish a true historical quote endpoint.

## Progress update (2026-03-07)

Status: complete for the scoped Bitget adapter surface.

Completed since the original MVP lock:
1. Product coverage expanded from Spot + USDT-FUTURES to Spot + USDT-FUTURES + COIN-FUTURES + USDC-FUTURES.
2. Public live quote ticks implemented.
3. Public live bars implemented.
4. Public live mark prices and index prices implemented.
5. Public live funding-rate updates implemented.
6. Historical bar requests implemented.
7. Current and historical funding-rate requests implemented.
8. Product-type routing no longer relies on USDT-only symbol heuristics; Python and Rust now use exact futures-family handling.
9. Trading REST flows and reconciliation/report flows now cover the supported Bitget product families.

Remaining non-goals:
1. Historical quote-tick requests.
2. UTA v3 private trading/account APIs.
1. Margin/borrow/lend, copy trading, bots, and affiliate endpoints.
1. Advanced exchange features (RFQ, block trades, algo orders beyond basic stop/trigger support).

## Symbology (Nautilus boundary)

Bitget reuses the same raw symbol strings across product lines (e.g., `BTCUSDT` spot and `BTCUSDT` USDT perps), so Nautilus must disambiguate.

**Proposed mapping (consistent with existing Binance suffix policy):**
1. **Spot**: keep native raw symbols (no suffix): `BTCUSDT.BITGET`.
1. **Perpetual futures**: append `-PERP`: `BTCUSDT-PERP.BITGET`.
1. **Delivery futures**: append `-YYMMDD` using `deliveryTime` (UTC date): `BTCUSDT-260626.BITGET`.

Implementation note: symbol parsing must be lossless and round-tripable:
- `InstrumentId.symbol` → determine product (`SPOT` vs `USDT-FUTURES`) and instrument kind (`PERP` vs delivery).
- Raw Bitget fields (symbol + productType + symbolType + deliveryTime) → deterministically generate the Nautilus `Symbol`.

## Reference implementations (source of truth)

Use these to match repo conventions for adapter layout + responsibilities:
1. Python adapter patterns (flat package): `nautilus_trader/adapters/okx`, `nautilus_trader/adapters/bybit`.
1. Rust adapter patterns (crate layout + PyO3 bindings + wiring): `crates/adapters/okx`, `crates/adapters/bybit`.
1. PyO3 aggregation wiring: `crates/pyo3/src/lib.rs`, `crates/pyo3/Cargo.toml`.
1. Integration docs patterns: `docs/integrations/bybit.md`, `docs/integrations/binance.md`, `docs/integrations/okx.md`.

## Bitget API notes (Classic v2)

1. **V1 is deprecated** (removed **November 28, 2025**): do not implement v1 endpoints or v1 symbol formats.
1. **REST base**: `https://api.bitget.com`.
1. **WS base**: public `wss://ws.bitget.com/v2/ws/public`, private `wss://ws.bitget.com/v2/ws/private`.
1. **Demo trading (Classic)**:
   - REST: add header `paptrading: 1` on every request, and use demo API keys.
   - WebSocket: use public `wss://wspap.bitget.com/v2/ws/public` and private `wss://wspap.bitget.com/v2/ws/private`.
1. **Auth headers** (REST): `ACCESS-KEY`, `ACCESS-SIGN`, `ACCESS-TIMESTAMP` (ms), `ACCESS-PASSPHRASE`.
1. **REST signature string**:
   - if no querystring: `timestamp + METHOD + requestPath + body`
   - if querystring present: `timestamp + METHOD + requestPath + "?" + queryString + body`
   - `METHOD` must be uppercase.
1. **WS private login signature**: `base64(hmac_sha256(timestamp + "GET" + "/user/verify", secret))` with timestamp in **milliseconds** (expires in ~30s).
1. **WS ops constraints**:
   - send `"ping"` every ~30s and expect `"pong"`.
   - server will disconnect if no ping for ~2 minutes.
   - connection is forcibly disconnected every ~24 hours; reconnect + resubscribe automatically.
   - message rate limit ~10 msg/sec per connection.
1. **Orderbook sequencing**:
   - depth pushes include `seq` which increments when the book updates (except during symbol maintenance where it may reset).
   - depth pushes include `checksum` (CRC32 of the top 25 levels) to validate your local book.

## Endpoint inventory (MVP, Classic v2)

Keep this list synced with the implementation. Prefer **REST for state reconciliation** and **WS for streaming**.

### Spot (REST)
1. Server time: `GET /api/v2/public/time` (use for clock skew detection)
1. Instruments: `GET /api/v2/spot/public/symbols`
1. Orderbook snapshot: `GET /api/v2/spot/market/orderbook`
1. Trades (historical pull): `GET /api/v2/spot/market/fills-history`
1. Bars: `GET /api/v2/spot/market/candles`
1. Account assets: `GET /api/v2/spot/account/assets`
1. Place order: `POST /api/v2/spot/trade/place-order`
1. Cancel order: `POST /api/v2/spot/trade/cancel-order`
1. Modify order (cancel+replace): `POST /api/v2/spot/trade/cancel-replace-order`
1. Cancel all for symbol: `POST /api/v2/spot/trade/cancel-symbol-order` (async; avoid if multi-strategy unsafe)
1. Batch cancel: `POST /api/v2/spot/trade/batch-cancel-order`
1. Open orders: `GET /api/v2/spot/trade/unfilled-orders`
1. Fills: `GET /api/v2/spot/trade/fills`

### Futures (USDT-FUTURES) (REST)
1. Instruments/config: `GET /api/v2/mix/market/contracts?productType=USDT-FUTURES`
1. Orderbook snapshot: `GET /api/v2/mix/market/merge-depth`
1. Trades (historical pull): `GET /api/v2/mix/market/fills-history`
1. Bars: `GET /api/v2/mix/market/candles`
1. Mark/index/last prices: `GET /api/v2/mix/market/symbol-price`
1. Funding rate: `GET /api/v2/mix/market/current-fund-rate`
1. Place order: `POST /api/v2/mix/order/place-order`
1. Cancel order: `POST /api/v2/mix/order/cancel-order`
1. Modify order: `POST /api/v2/mix/order/modify-order` (can async replace orderId; requires `newClientOid`)
1. Batch cancel: `POST /api/v2/mix/order/batch-cancel-orders`
1. Cancel all (broad): `POST /api/v2/mix/order/cancel-all-orders` (product-wide; do not use by default)
1. Open orders (pending): `GET /api/v2/mix/order/orders-pending`
1. Fills: `GET /api/v2/mix/order/fills`
1. Account: `GET /api/v2/mix/account/account`
1. Positions: `GET /api/v2/mix/position/all-position`

### WebSocket channels (Classic v2)
1. Public depth: `channel=books|books1|books5|books15` with `action=snapshot|update`, includes `seq` + `checksum`.
1. Public trades: `channel=trade` (spot + futures).
1. Private spot: `orders`, `fill`, and `account`.
1. Private futures: `orders`, `fill`, `positions`, and `account` (note: some futures channels only support `instId=default`).

---

### Task 1: Worktree setup + baseline checks

**Owner:** Lead agent.

**Files:**
1. Modify (if needed): `.gitignore`

**Step 1: Create a repo-local worktree dir**

Run in `/home/ubuntu/nautilus_trader`:
```bash
ls -d .worktrees 2>/dev/null || ls -d worktrees 2>/dev/null || true
```

**Step 2: Ensure it’s ignored**

If using `.worktrees/` or `worktrees/`:
```bash
git check-ignore -q .worktrees || git check-ignore -q worktrees
```

If not ignored, add to `.gitignore` and commit.

**Step 3: Create focused worktrees**
```bash
git worktree add .worktrees/bitget-rust -b feat/bitget-rust
git worktree add .worktrees/bitget-python -b feat/bitget-python
git worktree add .worktrees/bitget-docs -b feat/bitget-docs
```

**Step 4: Baseline sanity**

Run in each worktree:
```bash
python3 -V
pytest -q
make cargo-test-core
```

Expected: tests pass or failures are documented as pre-existing.

---

### Task 2: Decide supported product surface (lock scope early)

**Owner:** Lead agent.

**Files:**
1. Modify: `docs/plans/2026-03-03-bitget-exchange-integration.md`

**Step 1: Lock MVP product matrix**

**Final decision (updated):** Spot + USDT-FUTURES + COIN-FUTURES + USDC-FUTURES.

Pick one and keep it consistent throughout implementation:
1. **Selected:** Spot + USDT-FUTURES + COIN-FUTURES + USDC-FUTURES.
1. Spot-only (narrower but faster). Not selected.
1. Spot + all futures (USDT/USDC/COIN) (largest). Not selected.

Update the “Scope (MVP)” section accordingly and commit.

---

### Task 3: Rust crate scaffold (`nautilus-bitget`) + workspace wiring

**Owner:** Rust scaffold agent.

**Files:**
1. Create: `crates/adapters/bitget/Cargo.toml`
1. Create: `crates/adapters/bitget/README.md`
1. Create: `crates/adapters/bitget/src/lib.rs`
1. Create: `crates/adapters/bitget/src/python/mod.rs`
1. Modify: `Cargo.toml`
1. Modify: `crates/pyo3/Cargo.toml`
1. Modify: `crates/pyo3/src/lib.rs`
1. Create: `python/nautilus_trader/adapters/bitget/__init__.py`

**Step 1: Create the crate by copying the OKX skeleton**

Start by copying structure from `crates/adapters/okx` but remove OKX-specific logic.

Minimum Rust modules to create (may start empty):
- `common` (enums + shared models)
- `config` (Rust configs)
- `http` (client + REST models)
- `websocket` (client + WS models/parsers)
- `python` (PyO3 exports + registry wiring)

**Step 2: Wire workspace membership + dependency**

Add `crates/adapters/bitget` to `Cargo.toml` `[workspace].members` and add `nautilus-bitget` to `[workspace.dependencies]` (match existing adapter entries).

**Step 3: Wire PyO3 aggregation**

In `crates/pyo3/Cargo.toml`:
1. Add `nautilus-bitget` to `[dependencies]` with `features = ["python"]`.
1. Add `nautilus-bitget/extension-module` to `extension-module` feature list.
1. Add `nautilus-bitget/high-precision` to `high-precision` feature list (match other adapters).

In `crates/pyo3/src/lib.rs`, add:
```rust
let n = "bitget";
let submodule = pyo3::wrap_pymodule!(nautilus_bitget::python::bitget);
m.add_wrapped(submodule)?;
sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
#[cfg(feature = "cython-compat")]
re_export_module_attributes(m, n)?;
```

**Step 4: Add the python re-export shim for maturin builds**

Create `python/nautilus_trader/adapters/bitget/__init__.py`:
```python
from nautilus_trader._libnautilus.bitget import *  # noqa: F403
```

**Step 5: Build sanity**
```bash
make build-debug
```

Expected: build succeeds and the module `nautilus_trader._libnautilus.bitget` is importable.

**Step 6: Commit**
```bash
git add Cargo.toml crates/pyo3/Cargo.toml crates/pyo3/src/lib.rs crates/adapters/bitget python/nautilus_trader/adapters/bitget
git commit -m "feat(bitget): scaffold rust adapter crate and pyo3 wiring"
```

---

### Task 4: Rust “common” layer (enums, symbols, URLs, signing)

**Owner:** Rust core agent.

**Files:**
1. Create: `crates/adapters/bitget/src/common/mod.rs`
1. Create: `crates/adapters/bitget/src/common/enums.rs`
1. Create: `crates/adapters/bitget/src/common/symbol.rs`
1. Create: `crates/adapters/bitget/src/common/urls.rs`
1. Create: `crates/adapters/bitget/src/common/signing.rs`
1. Test: `crates/adapters/bitget/tests/signing.rs`

**Step 1: Define stable enums**

Implement at minimum:
1. `BitgetEnvironment` (MAINNET, DEMO).
1. `BitgetProductType` (SPOT, USDT_FUTURES, COIN_FUTURES, USDC_FUTURES) — even if MVP only uses a subset.
1. `BitgetInstrumentKind` (SPOT, PERP, DELIVERY).
1. `BitgetWsInstType` values used in `arg.instType` (SPOT, USDT-FUTURES, ...).
1. `BitgetOrderSide` (buy/sell), `BitgetOrderType` (limit/market), `BitgetTimeInForce` mapping.

**Step 2: Encode symbology rules**

In `symbol.rs`, implement:
1. `fn nautilus_symbol_for_spot(raw: &str) -> String`
1. `fn nautilus_symbol_for_perp(raw: &str) -> String` (append `-PERP`)
1. `fn nautilus_symbol_for_delivery(raw: &str, delivery_time_ms: i64) -> String` (append `-YYMMDD`)
1. `fn parse_nautilus_symbol(symbol: &str) -> BitgetInstrumentKind` (PERP suffix, YYMMDD pattern, else SPOT)

Add unit tests which round-trip: raw → nautilus → parse-kind.

**Step 3: URL helpers**

In `urls.rs` implement:
1. `get_http_base_url(env) -> Url`
1. `get_ws_public_url(env) -> Url`
1. `get_ws_private_url(env) -> Url`

**Step 4: Signing helpers**

In `signing.rs` implement:
1. REST signing helper: `(timestamp_ms, method, request_path, query_string, body_bytes) -> sign_b64`, where `query_string` excludes the leading `?` (the helper inserts `?` when present).
1. WS login signing helper: `(ts) -> sign_b64` for `GET /user/verify`.

In tests:
1. Assert the “string to sign” matches Bitget’s docs examples (GET with querystring + POST with JSON body).
1. Compute one expected base64 signature out-of-band (e.g., `python3 -c ...`) and assert the Rust signing helper matches it.

Example expected-signature generator (use the output as a hard-coded expected value in tests):
```bash
python3 - <<'PY'
import base64, hmac, hashlib
secret=b"testsecret"
payload="1659927630000GET/api/mix/v2/market/depth?limit=20&symbol=BTCUSDT"
print(base64.b64encode(hmac.new(secret, payload.encode(), hashlib.sha256).digest()).decode())
PY
```
Expected output:
```text
BUbassOUwHdAjeOFu8D5FZfp4i2JGYCsS9yBvRLaC0U=
```

**Step 5: Commit**
```bash
git add crates/adapters/bitget/src/common crates/adapters/bitget/tests/signing.rs
git commit -m "feat(bitget): add common enums, urls, and signing"
```

---

### Task 5: Rust HTTP client (public endpoints + instruments)

**Owner:** Rust HTTP agent.

**Files:**
1. Create: `crates/adapters/bitget/src/http/mod.rs`
1. Create: `crates/adapters/bitget/src/http/client.rs`
1. Create: `crates/adapters/bitget/src/http/models.rs`
1. Create: `crates/adapters/bitget/test_data/http_spot_symbols.json`
1. Create: `crates/adapters/bitget/test_data/http_contract_config.json`
1. Test: `crates/adapters/bitget/tests/http_public.rs`

**Step 1: Implement a minimal async `BitgetHttpClient`**

Follow patterns in `crates/adapters/okx/src/http/client.rs`:
1. Store credentials + env + base_url + rate limiting.
1. Expose `new(...)` and `with_credentials(...)` constructors.
1. Implement `cache_instrument(...)` for WS subscriptions.

**Step 2: Implement public instrument fetch**

Implement:
1. Spot symbols: `GET /api/v2/spot/public/symbols`
1. Contract config: `GET /api/v2/mix/market/contracts?productType=USDT-FUTURES` (and other product types later)

Parse into Nautilus model instruments (`CurrencyPair`, `CryptoPerpetual`, `CryptoFuture`) as PyO3-exportable types (match OKX/Bybit patterns).

**Step 3: Add fixtures + tests**

Save real responses (redacted if needed) into `test_data/` and write tests which:
1. Deserialize response models.
1. Build instruments and assert required fields are non-empty (precision, increments, currencies).
1. Validate delivery futures symbol construction uses `deliveryTime`.

**Step 4: Commit**
```bash
git add crates/adapters/bitget/src/http crates/adapters/bitget/test_data crates/adapters/bitget/tests/http_public.rs
git commit -m "feat(bitget): add http client and instrument parsing"
```

---

### Task 6: Rust WebSocket (public market data)

**Owner:** Rust WS agent.

**Files:**
1. Create: `crates/adapters/bitget/src/websocket/mod.rs`
1. Create: `crates/adapters/bitget/src/websocket/client.rs`
1. Create: `crates/adapters/bitget/src/websocket/messages.rs`
1. Create: `crates/adapters/bitget/src/websocket/parse.rs`
1. Modify: `crates/adapters/bitget/Cargo.toml` (add CRC32 dependency if needed)
1. Create: `crates/adapters/bitget/test_data/ws_public_depth_snapshot.json`
1. Create: `crates/adapters/bitget/test_data/ws_public_depth_update.json`
1. Create: `crates/adapters/bitget/test_data/ws_public_trades.json`
1. Test: `crates/adapters/bitget/tests/ws_public_parse.rs`

**Step 1: Implement WS connect + subscribe**

Follow patterns in `crates/adapters/okx/src/websocket/client.rs`:
1. `connect(loop, instruments, callback)` for PyO3 usage.
1. `wait_until_active(timeout_secs)`.
1. `subscribe_*` / `unsubscribe_*` helpers which send:
```json
{"op":"subscribe","args":[{"instType":"SPOT","channel":"books","instId":"BTCUSDT"}]}
```

**Step 2: Parse depth + trades into Nautilus `Data` capsules**

Implement parsing for:
1. Spot depth channel.
1. Contract depth channel (USDT-FUTURES).
1. Trades channel (spot + contract).

Emit `OrderBookDelta` / `TradeTick` via the existing Rust→PyCapsule pathway used by OKX/Bybit.

**Step 3: Bookbuilding rules**

Implement:
1. Accept `snapshot` then incremental `update`.
2. Track `seq` and detect out-of-order packets; handle resets during maintenance by forcing a fresh snapshot.
3. Validate `checksum` (CRC32) against the local top-25 book string; on mismatch, request REST snapshot and rebuild:
   - build the checksum string by alternating best bid/ask per level: `bid1Px:bid1Sz:ask1Px:ask1Sz:bid2Px:...` (then append remaining side if one side has fewer than 25).
   - use the **raw string values** received from Bitget (do not reformat floats / trim trailing zeros) when building the checksum string.
   - implement CRC32 using a standard crate (e.g., `crc32fast`) and match Bitget’s expected integer semantics.
4. Prefer `books` (full depth) for bookbuilding; `books1|books5|books15` snapshots are useful for lightweight strategies but are not “delta streams”.

**Step 4: Tests**

Using fixtures, assert:
1. Snapshot parses into a valid initial book state.
2. Update produces deltas that mutate the book deterministically.
3. Trades parse into `TradeTick` with correct side + price/size precision.

**Step 5: Commit**
```bash
git add crates/adapters/bitget/src/websocket crates/adapters/bitget/test_data crates/adapters/bitget/tests/ws_public_parse.rs
git commit -m "feat(bitget): add public websocket parsing for depth and trades"
```

---

### Task 7: Rust WebSocket (private execution + account streams)

**Owner:** Rust WS-exec agent.

**Files:**
1. Modify: `crates/adapters/bitget/src/websocket/client.rs`
1. Modify: `crates/adapters/bitget/src/websocket/messages.rs`
1. Create: `crates/adapters/bitget/test_data/ws_private_login.json`
1. Create: `crates/adapters/bitget/test_data/ws_private_orders.json`
1. Create: `crates/adapters/bitget/test_data/ws_private_fills.json`
1. Create: `crates/adapters/bitget/test_data/ws_private_account.json`
1. Create: `crates/adapters/bitget/test_data/ws_private_positions.json`
1. Test: `crates/adapters/bitget/tests/ws_private_parse.rs`

**Step 1: Implement WS private login**

Send:
```json
{"op":"login","args":[{"apiKey":"...","passphrase":"...","timestamp":"...","sign":"..."}]}
```

**Step 2: Subscribe to private channels**

Implement parsing and callbacks for:
1. Spot orders channel.
1. Contract orders channel.
1. Spot fills channel.
1. Contract fills channel.
1. Spot account channel.
1. Contract account + positions channels.

Notes:
1. Some contract channels only support `instId=default` (not per-symbol subscriptions); implement that constraint explicitly rather than silently failing subscriptions.

**Step 3: Map to Nautilus execution reports**

Prefer emitting:
1. `OrderStatusReport` updates.
1. `FillReport` updates.
1. `PositionStatusReport` updates.

If full mapping is too large initially, emit typed Rust models first, then map in Python execution client (but keep a clear migration path back to Rust-side normalized reports).

**Step 4: Commit**
```bash
git add crates/adapters/bitget/test_data crates/adapters/bitget/tests/ws_private_parse.rs crates/adapters/bitget/src/websocket
git commit -m "feat(bitget): add private websocket parsing for orders, fills, and account state"
```

---

### Task 8: Rust adapter configs/factories + PyO3 module exports

**Owner:** Rust PyO3 agent.

**Files:**
1. Create: `crates/adapters/bitget/src/config.rs`
1. Create: `crates/adapters/bitget/src/factories.rs`
1. Create: `crates/adapters/bitget/src/python/config.rs`
1. Create: `crates/adapters/bitget/src/python/enums.rs`
1. Create: `crates/adapters/bitget/src/python/factories.rs`
1. Create: `crates/adapters/bitget/src/python/http.rs`
1. Create: `crates/adapters/bitget/src/python/models.rs`
1. Create: `crates/adapters/bitget/src/python/urls.rs`
1. Create: `crates/adapters/bitget/src/python/websocket.rs`
1. Modify: `crates/adapters/bitget/src/python/mod.rs`

**Step 1: Implement Rust config types**

Mirror other adapters:
1. `BitgetDataClientConfig` (env, product types, URLs, retries, instrument refresh interval).
1. `BitgetExecClientConfig` (env, trading flags, retry policy, account/position preferences).

Ensure they are PyO3-exportable for stub generation.

**Step 2: Implement factories**

Add `BitgetDataClientFactory` and `BitgetExecutionClientFactory` in Rust (even if Python live clients are used for now) to match repo patterns and to support Rust-only runs.

**Step 3: Export in `nautilus_pyo3.bitget`**

In `crates/adapters/bitget/src/python/mod.rs`:
1. `m.add_class::<BitgetHttpClient>()?;`
1. `m.add_class::<BitgetWebSocketClient>()?;`
1. Add enums, models, config + factories.
1. Export URL helpers.
1. Register factory/config extractors with `get_global_pyo3_registry()` using the key `"BITGET"`.

**Step 4: Stub generation**
```bash
cd python
python3 generate_stubs.py
```

Expected: new Bitget types appear in generated `.pyi` output.

**Step 5: Commit**
```bash
git add crates/adapters/bitget/src/config.rs crates/adapters/bitget/src/factories.rs crates/adapters/bitget/src/python
git commit -m "feat(bitget): add rust configs/factories and pyo3 exports"
```

---

### Task 9: Python adapter package (`nautilus_trader.adapters.bitget`)

**Owner:** Python adapter agent.

**Files:**
1. Create: `nautilus_trader/adapters/bitget/__init__.py`
1. Create: `nautilus_trader/adapters/bitget/constants.py`
1. Create: `nautilus_trader/adapters/bitget/config.py`
1. Create: `nautilus_trader/adapters/bitget/types.py`
1. Create: `nautilus_trader/adapters/bitget/providers.py`
1. Create: `nautilus_trader/adapters/bitget/factories.py`
1. Create: `nautilus_trader/adapters/bitget/data.py`
1. Create: `nautilus_trader/adapters/bitget/execution.py`

**Step 1: Mirror OKX/Bybit package layout**

Implement the minimum set of files (flat module structure) following:
- `nautilus_trader/adapters/okx/*`
- `nautilus_trader/adapters/bybit/*`

**Step 2: Instrument provider**

`BitgetInstrumentProvider` should:
1. Load spot + contract instruments via the Bitget HTTP client.
1. Build correct Nautilus instrument IDs with venue `BITGET_VENUE`.
1. Enforce `instrument_id.venue == BITGET_VENUE` in `load_ids_async`.
1. Support periodic refresh via `update_instruments_interval_mins`.

**Step 3: Live data client**

`BitgetDataClient(LiveMarketDataClient)` should:
1. Initialize provider, cache instruments, and connect WS public.
1. Handle pycapsule messages via `capsule_to_data(msg)` like OKX/Bybit.
1. Implement subscribe/unsubscribe for:
   - order book deltas
   - trade ticks
1. Implement REST requests for:
   - instruments
   - order book snapshot
   - bars
   - funding rates / prices (futures)

**Step 4: Live execution client**

`BitgetExecutionClient(LiveExecutionClient)` should:
1. Connect WS private (login + subscriptions).
1. Use REST for submit/cancel/modify/batch-cancel/cancel-all.
1. Consume private WS order/fill/account/position updates and publish:
   - `OrderStatusReport`
   - `FillReport`
   - `PositionStatusReport`
1. Provide reconciliation on startup (`generate_mass_status`) using REST open orders + positions.

**Step 5: Commit**
```bash
git add nautilus_trader/adapters/bitget
git commit -m "feat(bitget): add python adapter (provider, data, execution, factories)"
```

---

### Task 10: Python tests (adapter-level integration tests with mocks)

**Owner:** Python tests agent.

**Files:**
1. Create: `tests/integration_tests/adapters/bitget/conftest.py`
1. Create: `tests/integration_tests/adapters/bitget/test_providers.py`
1. Create: `tests/integration_tests/adapters/bitget/test_execution.py`

**Step 1: Copy OKX test layout**

Use `tests/integration_tests/adapters/okx/*` as a template.

**Step 2: Add provider tests**

Test:
1. `load_all_async` adds instruments to provider.
1. `load_ids_async` enforces venue correctness.
1. Symbol disambiguation rules (`-PERP`, `-YYMMDD`) produce correct instrument kinds.

**Step 3: Add execution tests**

Mock `nautilus_pyo3.BitgetHttpClient` and `nautilus_pyo3.BitgetWebSocketClient` and assert:
1. Submit order routes to correct REST method based on instrument kind.
1. Cancel/modify calls use correct REST endpoints.
1. Private WS messages trigger report publishing (at least one representative case).

**Step 4: Run tests**
```bash
pytest tests/integration_tests/adapters/bitget -q
```

Expected: all Bitget adapter tests pass.

**Step 5: Commit**
```bash
git add tests/integration_tests/adapters/bitget
git commit -m "test(bitget): add provider and execution integration tests"
```

---

### Task 11: Examples (live data + exec testers)

**Owner:** Examples agent.

**Files:**
1. Create: `examples/live/bitget/bitget_data_tester.py`
1. Create: `examples/live/bitget/bitget_exec_tester.py`
1. Create: `examples/live/bitget/README.md`

**Step 1: Copy the Binance/Bybit tester patterns**

Use as references:
- `examples/live/binance/binance_spot_exec_tester.py`
- `examples/live/bybit/bybit_exec_tester.py`

**Step 2: Document env vars**

Document:
1. `BITGET_API_KEY`, `BITGET_API_SECRET`, `BITGET_API_PASSPHRASE`
1. Demo equivalents (if you choose to support separate env vars), otherwise use config fields + `demo=True`.
1. Required account permissions (read-only vs trade).

**Step 3: Runbook**

Provide commands to run spot market data and place/cancel a single limit order in demo.

**Step 4: Commit**
```bash
git add examples/live/bitget
git commit -m "docs(bitget): add live example scripts"
```

---

### Task 12: Documentation + integration registry updates

**Owner:** Docs agent.

**Files:**
1. Create: `docs/integrations/bitget.md`
1. Modify: `docs/integrations/index.md`
1. Modify: `README.md`
1. Optional: `docs/api_reference/adapters/bitget.md`
1. Optional: `docs/api_reference/adapters/index.md`

**Step 1: Write `docs/integrations/bitget.md`**

Mirror structure from `docs/integrations/bybit.md`:
1. Overview + component list.
1. Products table (Spot, USDT-FUTURES; plus placeholders for future COIN/USDC).
1. Symbology rules with concrete examples (`BTCUSDT.BITGET`, `BTCUSDT-PERP.BITGET`, `BTCUSDT-260626.BITGET`).
1. Environments (mainnet vs demo/paper trading).
1. Orders capability tables (order types, TIF, post-only/reduce-only support).
1. Known limitations (WS “place-order” permissions; release window disconnects).

**Step 2: Add Bitget to integration tables**

Update:
1. `docs/integrations/index.md` supported integrations table (alphabetical).
1. `README.md` supported integrations table (alphabetical).

**Step 3: Commit**
```bash
git add docs/integrations/bitget.md docs/integrations/index.md README.md
git commit -m "docs(bitget): add integration guide and registry entries"
```

---

### Task 13: Verification gates (do not skip)

**Owner:** Lead agent.

**Step 1: Format/lint**
```bash
make format
make check-code
```

**Step 2: Rust tests**
```bash
make cargo-test-crate-nautilus-bitget
```

**Step 3: Python tests**
```bash
pytest tests/integration_tests/adapters/bitget -q
pytest tests/unit_tests -q
```

**Step 4: Build**
```bash
make build-debug
```

Expected: build succeeds, and imports work:
```bash
python3 -c "import nautilus_trader; import nautilus_trader.adapters.bitget"
```

---

### Task 14: Acceptance criteria

**End-to-end (demo)**
1. Instruments load for Spot and USDT-FUTURES.
2. Subscribe to `books` + `trade` for at least one instrument in each product (e.g., `BTCUSDT` spot and `BTCUSDT-PERP` futures) and receive continuous data for 10 minutes with no unhandled exceptions.
3. Place, cancel, and (if supported) modify a limit order in demo; Nautilus receives:
   - order accepted/open update
   - order canceled update (or fill if crossed)
4. On forced WS disconnect (24h) and on manual network interruption, clients reconnect and resubscribe automatically.

**Docs/examples**
1. `docs/integrations/bitget.md` exists and is linked from the integrations index and README.
2. `examples/live/bitget/*` scripts run with documented env vars and configs.
