# dYdX Rust Data Adapter – Data Parity & Standardization Plan

> Scope: Rust-only, **data layer only** for the dYdX adapter.
> Non‑goals: changing Python code, modifying execution/grpc logic, or altering core framework APIs beyond what’s strictly required for dYdX data parity.

---

## 1. Objectives & Constraints

- Ensure the **Rust dYdX data adapter**:
  - Follows `docs/developer_guide/adapters.md` and matches patterns from OKX, BitMEX, Bybit.
  - Achieves **functional parity** with the Python `DYDXDataClient` for all data‑related features.
  - Remains consistent with the official `v4-clients/v4-client-rs` Indexer client where applicable (HTTP/WebSocket semantics).
- Keep focus strictly on:
  - `crates/adapters/dydx/src/data` (and its dependencies: `common`, `http`, `websocket`, `config`, `types`, `schemas`).
  - Rust bindings in `crates/adapters/dydx/src/python` only as needed for data integration.
- Do **not**:
  - Modify Python implementation (can use it as a specification and for tests).
  - Touch non‑dYdX adapters except for reading patterns (OKX/BitMEX/Bybit/Hyperliquid).
  - Introduce `/// # Arguments`, `/// # Examples` style comments.

---

## 2. Baseline & Reference Patterns

### 2.1 Adapter guide invariants (docs/developer_guide/adapters.md)

- **Layered architecture**:
  - Rust core at `crates/adapters/<venue>/`:
    - `common/` (consts, credential helpers, enums, urls, parse, testing).
    - `http/` (raw + domain HTTP clients, models, query builders, parse).
    - `websocket/` (WS client, messages, parse, enums).
    - `data/` (Rust `DataClient` implementation).
    - `execution/` (execution client – out of scope for this plan).
    - `config.rs`, `error.rs`, `lib.rs`, `python/`.
- **Data client pattern**:
  - One `*DataClient` struct per venue implementing `nautilus_data::client::DataClient`.
  - Uses `get_data_event_sender()` to emit `DataEvent::Data` and `DataEvent::Response`.
  - Handles:
    - Lifecycle: `start`, `stop`, `reset`, `dispose`, `connect`, `disconnect`, `is_connected`, `is_disconnected`.
    - Subscriptions: `subscribe_*`/`unsubscribe_*` for trades, books, quotes, bars, etc.
    - Requests: `request_instruments`, `request_instrument`, `request_trades`, `request_bars`, etc.
  - Instrument bootstrapping via HTTP + WS caching and periodic refresh.
- **HTTP/WS client patterns**:
  - Two‑layer HTTP (`RawHttpClient` + `HttpClient`) using `nautilus_network::http::HttpClient`.
  - WebSocket client using `nautilus_network::websocket::WebSocketClient` and venue‑specific `NautilusWsMessage`.
- **Style**:
  - License block header at top of each file.
  - One concise `//!` module doc, with focused per‑function docs where needed.
  - Logging via `tracing` with consistent structured fields.

### 2.2 Reference Rust adapters (OKX, BitMEX, Bybit)

- **Data clients**:
  - `crates/adapters/okx/src/data/mod.rs:OKXDataClient`
  - `crates/adapters/bitmex/src/data/mod.rs:BitmexDataClient`
  - `crates/adapters/bybit/src/data/mod.rs` (currently mostly a stub but follows header/doc style).
- Common patterns to mirror in dYdX:
  - Struct layout:
    - `client_id`, `config`, HTTP/WS clients, `is_connected`, `cancellation_token`, `tasks`.
    - Instrument cache: `Arc<RwLock<AHashMap<InstrumentId, InstrumentAny>>>` (OKX/BitMEX).
    - Book‑specific maps: `book_channels` (OKX/BitMEX) for depth variants.
  - Lifecycle:
    - `start` logs configuration; `stop` cancels token, resets state.
    - `connect` bootstraps instruments, connects WS, spawns stream tasks, starts instrument refresh.
    - `disconnect` unsubscribes, closes WS, drains tasks, resets caches.
  - WS handling:
    - `handle_ws_message` routing `NautilusWsMessage` → data events, instrument updates, ignoring execution messages.
    - `spawn_ws` helpers that wrap `Future<Output = anyhow::Result<()>>` with error logging.
    - One or more stream tasks per WS (public/business).
  - Requests:
    - `request_instruments` and `request_instrument` use HTTP domain client and instrument cache.
    - `request_trades`/`request_bars` delegate to HTTP, then wrap in `DataResponse::Trades/Bars`.
  - Subscriptions:
    - Guard against unsupported book types (e.g., only `L2_MBP` for many venues).
    - Keep local bookkeeping for subscribed instruments where needed.

### 2.3 Current Rust dYdX data implementation snapshot

- Main data implementation: `crates/adapters/dydx/src/data/mod.rs`.
  - Implements full `DataClient` trait (trades, books, quotes, bars, instruments).
  - Uses:
    - HTTP client: `crates/adapters/dydx/src/http/client.rs:DydxHttpClient`.
    - WS client: `crates/adapters/dydx/src/websocket/client.rs:DydxWebSocketClient`.
    - Common utilities: `crates/adapters/dydx/src/common/`.
  - Key structural differences from OKX/BitMEX:
    - Instrument cache keyed by `Ustr` symbol (`DashMap<Ustr, InstrumentAny>`), not `InstrumentId`.
    - Rich local state:
      - `order_books: Arc<DashMap<InstrumentId, OrderBook>>`
      - `last_quotes: Arc<DashMap<InstrumentId, QuoteTick>>`
      - `incomplete_bars: Arc<DashMap<BarType, Bar>>`
      - `bar_type_mappings: Arc<DashMap<String, BarType>>`
      - `active_orderbook_subs: Arc<DashMap<InstrumentId, ()>>`
    - Background tasks:
      - Instrument refresh (`start_instrument_refresh_task`).
      - Orderbook snapshot refresh (`start_orderbook_refresh_task`).
  - WS handling:
    - Uses `DydxWebSocketClient::take_receiver()` to get a channel of `NautilusWsMessage`.
    - `handle_ws_message` routes:
      - `Data(Vec<Data>)` → `DataEvent::Data`.
      - `Deltas(OrderBookDeltas)` → crossed‑book resolution + quote generation + deltas emission.
      - `OraclePrices` → conversion to `DydxOraclePrice` (currently logged, not emitted as `Data`).
      - Execution/subaccount messages → ignored (logged at debug).
  - Requests:
    - `request_instrument`:
      - Cache‑first from `DashMap<Ustr, InstrumentAny>`, fallback to HTTP `request_instruments`.
      - Sends `DataResponse::Instrument(Box<InstrumentResponse>)`.
    - `request_instruments`:
      - HTTP `request_instruments` (no filters yet) → caches in `DashMap` + emits `InstrumentsResponse`.
    - `request_trades`:
      - HTTP `get_trades` with symbol (strip `-PERP`), limit only.
      - Converts to `TradeTick`, filters by optional `start`/`end` timestamps at the client side.
    - `request_bars`:
      - Validates EXTERNAL aggregation, time bars, `PriceType::Last`.
      - Maps `BarType` to `DydxCandleResolution`.
      - Implements partitioning into ≤1000‑bar chunks, excludes incomplete current bar.
  - Additional features:
    - Crossed orderbook resolution (`resolve_crossed_order_book`).
    - Quote generation from orderbook deltas with fallback to last quote.
    - HTTP orderbook snapshot → deltas conversion for periodic refresh.
    - Extensive unit tests embedded in `data/mod.rs` (book resolution, requests, WS handling).

### 2.4 Current Python dYdX data behaviour (spec only)

- `nautilus_trader/adapters/dydx/data.py:DYDXDataClient`:
  - Handles:
    - Subscriptions: trades, orderbook, candles, quotes (synthesized), instruments/markets.
    - HTTP requests for trades, bars (with partitioning and incomplete‑bar filtering).
    - Crossed orderbook resolution (`_resolve_crossed_order_book`) and quote generation.
    - Periodic HTTP orderbook snapshots for subscribed instruments.
    - Oracle prices: publishes `DYDXOraclePrice` messages via `MessageBus` using `DataType(DYDXOraclePrice)`.
  - Uses `DYDXInstrumentProvider` for instrument bootstrapping and currency loading.
  - Serves as de‑facto behavioural spec for parity (especially around edge cases and data semantics).

### 2.5 v4-client-rs baseline

- Official dYdX v4 client in `v4-clients/v4-client-rs/client`:
  - Indexer REST: `client/src/indexer/rest/*`.
  - WebSocket: `client/src/indexer/sock/*`.
- Provides reference for:
  - REST endpoint coverage, query parameters, and pagination semantics.
  - WebSocket channel names, subscription payloads, and reconnection patterns.
- Plan must ensure our `DydxRawHttpClient`/`DydxHttpClient` and `DydxWebSocketClient` follow compatible semantics, especially for data fields used by the data client.

---

## 3. Gap Analysis – dYdX Rust vs Reference Adapters & Python

### 3.1 Interface coverage (DataClient trait)

- Implemented in Rust and Python:
  - `request_instruments`, `request_instrument`, `request_trades`, `request_bars`.
  - `subscribe_instruments` / `unsubscribe_instruments` (no‑ops in Rust due to global markets channel).
  - `subscribe_instrument` / `unsubscribe_instrument` (no‑ops).
  - `subscribe_trades` / `unsubscribe_trades`.
  - `subscribe_book_deltas` / `unsubscribe_book_deltas`.
  - `subscribe_book_snapshots` / `unsubscribe_book_snapshots`.
  - `subscribe_bars` / `unsubscribe_bars`.
  - `subscribe_quotes` / `unsubscribe_quotes` (delegated to book deltas).
  - Lifecycle: `start`, `stop`, `reset`, `dispose`, `connect`, `disconnect`, `is_connected`, `is_disconnected`.
- Not implemented in Rust (and effectively not meaningful for dYdX):
  - Mark/index prices, funding rates, depth10, instrument status/close.
  - These are optional or missing from the dYdX Indexer API and reference Python implementation.
- **Actionable gap**:
  - Document and codify which DataClient methods are intentionally unsupported for dYdX, matching `DYDX_DATA_PLAN.md` and adapter guide expectations.
  - Ensure unimplemented methods are either:
    - Explicit no‑ops with clear logging, or
    - Not part of the trait surface for dYdX (if trait allows).

### 3.2 Live streaming behaviour (WS) vs Python

- Both Rust and Python:
  - Use a single “markets” channel for instruments/oracle prices.
  - Map `v4_trades`, `v4_orderbook`, `v4_candles` channels to TradeTick / OrderBookDeltas / Bar.
  - Treat “subscribed” messages as non‑data control messages (used for snapshot bootstrapping).
  - Implement crossed orderbook resolution and quote generation from deltas.
  - Maintain per‑instrument orderbook and last quote state.
- Observed differences / risks:
  - Rust WS handler currently logs oracle price events and creates `DydxOraclePrice` but does **not** forward as `DataEvent::Data` (blocked on `nautilus_model::data::Data` not supporting custom types), while Python publishes `DYDXOraclePrice` via `MessageBus`.
  - Rust uses `DashMap` + `OrderBook` with specific semantics; we must confirm they match Python’s orderbook application (e.g., snapshot + deltas ordering, flags).
  - Reconnection handling in Rust logs reconnected and active subscription counts; Python explicitly resubscribes on reconnect. Need to ensure dYdX WS client handles resubscriptions equivalent to Python.

### 3.3 Historical data semantics (trades & bars)

- Trades:
  - Both languages use Indexer trades endpoint with:
    - Symbol without `-PERP` suffix.
    - Limit parameter to bound results.
    - Client‑side filtering by start/end timestamps (since API is limit‑based).
  - Rust’s `request_trades`:
    - Validates instrument presence in cache; on miss: emits empty response with logging.
    - Converts to `TradeTick` using instrument precision; filters by `start_nanos`/`end_nanos`.
    - Builds `TradesResponse` with metadata consistent with adapter guide.
- Bars:
  - Both languages:
    - Require EXTERNAL time aggregation and `PriceType::Last`.
    - Map BarType spec → dYdX candle resolution (1/5/15/30 minutes, 1/4 hours, 1 day).
    - Split large ranges into ≤1000‑bar chunks, respecting overall limit.
    - Exclude incomplete current bar (ts_event >= “now”).
  - Rust uses `DydxCandleResolution` + `candle_to_bar` helper; Python uses `get_interval_from_bar_type` + `parse_to_bar`.
- **Actionable gaps**:
  - Confirm that:
    - Rust and Python use identical resolution mapping, especially around edge cases (4H, 1D).
    - Partitioning logic in Rust matches Python’s chunking behaviour for overlapping ranges.
  - Add parity tests (Rust vs Python expectations) for:
    - Limits, ranges, incomplete bars, “no data” responses.

### 3.4 Instruments & instrument provider parity

- Python:
  - Uses `DYDXInstrumentProvider` + `DYDXHttpClient` for instrument discovery.
  - Sends all instruments + currencies to DataEngine on startup via `_send_all_instruments_to_data_engine`.
- Rust:
  - `DydxDataClient::bootstrap_instruments` calls `http_client.request_instruments(None, None, None)` and:
    - Relies on HTTP client’s shared `DashMap<Ustr, InstrumentAny>` instrument cache.
    - Optionally caches instruments in WS client.
  - `request_instrument` and `request_instruments` rely on this shared cache.
- **Actionable gaps**:
  - Verify that Rust’s `DydxHttpClient::request_instruments` produces the **same instrument set and metadata** as Python’s `DYDXInstrumentProvider` / `DYDXHttpClient`.
  - Ensure currencies handling (e.g., collateral/stablecoin metadata) is either:
    - Not required on the Rust side, or
    - Appropriately mirrored if DataClient is expected to manage currencies.

### 3.5 Oracle price handling

- Python:
  - Parses oracle prices from markets channel and publishes `DYDXOraclePrice` to `MessageBus`.
- Rust:
  - `handle_oracle_prices` converts to `DydxOraclePrice` (Rust type under `crate::types`), logs them, and notes a TODO about forwarding once `Data` supports custom types.
- **Actionable gap**:
  - Decide on how oracle prices should surface in Rust:
    - Extend `nautilus_model::data::Data` (larger cross‑adapter change, likely out of immediate scope).
    - Or treat them as adapter‑specific events accessible via Python bindings rather than `DataClient`.
  - Update docs to clearly state the behaviour difference vs Python until unified.

### 3.6 Consistency with v4-client-rs

- HTTP:
  - Confirm that `DydxRawHttpClient` endpoints/queries match `v4-client-rs` Indexer REST client (markets, trades, candles, orderbook).
- WebSocket:
  - Confirm that subscription messages and channel names in `DydxWebSocketClient` match `v4-client-rs` socket client.
- **Actionable gap**:
  - Any divergence (e.g., field names, pagination flags, error handling) should be aligned or documented, prioritizing correctness and backward compatibility for Nautilus.

### 3.7 Tests & tooling

- Rust:
  - Extensive unit tests embedded in `dydx/src/data/mod.rs` plus HTTP tests.
  - Some tests are currently constrained by global singletons (`get_data_event_sender`) and async runtime setup, as documented in `DYDX_DATA_TESTING.md`.
- Python:
  - Multiple unit and integration tests under `tests/unit_tests/adapters/dydx` and `tests/integration_tests/adapters/dydx` serve as behavioural references.
- **Actionable gaps**:
  - Normalize Rust tests to avoid singleton and runtime issues.
  - Add parity tests comparing Rust responses to Python expectations (via shared fixtures or golden files).
  - Ensure running `make cargo-clippy`, `make pre-commit`, and test suites is part of the final verification.

---

## 4. Implementation Plan – Phases & Work Items

### Phase 0 – Planning & Safety Net

1. **Document current behaviour**
   - Lock in this plan in `PLAN.md` (done) and keep `DYDX_DATA_PLAN.md` / `DYDX_DATA_TESTING.md` as living status documents.
2. **Set guardrails**
   - No changes to Python code; treat Python behavioural tests as oracle where possible.
   - Avoid touching non‑data parts of the dYdX adapter (execution, grpc) unless a data bug is clearly rooted there.
3. **Baseline build & tests (when implementing)**
   - Run `cargo test -p adapters-dydx` (or equivalent workspace filters) to understand current breakage.
   - Run `make cargo-clippy` and `make pre-commit` once modifications are made to ensure style/tooling alignment.

### Phase 1 – Structural & Style Alignment (Rust dYdX vs OKX/BitMEX/Bybit)

1. **Module and file layout audit**
   - Compare dYdX `common`, `http`, `websocket`, `data`, `config`, `error`, `python` structure against OKX/BitMEX/Bybit.
   - Confirm:
     - `common/` has `consts`, `credential`, `enums`, `urls`, `parse`, `testing` in line with guide (already largely true).
     - `http/` has `client`, `error`, `models`, `parse`, `query` mirroring reference pattern.
     - `websocket/` has `client`, `messages`, `parse`, `enums`, `types`.
     - `data/` module only depends on these layers + `nautilus_*` crates.
   - Identify any remaining files that are out‑of‑place, and plan minimal moves/renames if absolutely necessary (while preserving public API).

2. **Header & module doc consistency**
   - Ensure all dYdX Rust files have:
     - Standard LGPL license header block identical to OKX/BitMEX.
     - A concise `//!` module‑level doc summarizing purpose, following adapter guide tone.
   - Trim overly long module docs if they diverge from project style, but keep high‑value details in comments or external MD docs where appropriate.

3. **Struct and field documentation**
   - Align `DydxDataClient` field comments with style used in OKX/BitMEX:
     - Keep essential explanations (e.g., orderbook/quote caches, active subscription tracking).
     - Avoid redundant comments that restate type names only.
   - Remove or adjust any doc fragments that violate the “no `/// # Arguments/Examples`” instruction.

4. **Helper naming and layout**
   - Align helper method naming with reference adapters:
     - `spawn_ws`, `spawn_stream_task`, `bootstrap_instruments`, `maybe_spawn_instrument_refresh` equivalents already exist; ensure naming is consistent and discoverable.
     - Group WS handling helpers (`handle_ws_message`, `handle_data_message`, `handle_deltas_message`, `handle_oracle_prices`) together and clearly separated from request handlers.

5. **Config struct alignment**
   - Compare `DydxDataClientConfig` to `OKXDataClientConfig`/`BitmexDataClientConfig`:
     - Ensure naming, defaulting, and semantics of `*_timeout_secs`, retry fields, and proxy URLs are consistent.
   - If necessary, add or adjust fields to match common config pattern while preserving existing behaviour.

### Phase 2 – DataClient Behaviour Parity (Rust vs Python)

1. **SUBSCRIBE/UNSUBSCRIBE mapping parity**
   - Build a matrix for each `Subscribe*` / `Unsubscribe*` method:
     - Inputs: command struct fields (instrument_id, venue, book_type, depth, params).
     - Outputs: WS subscription/unsubscription calls, changes to local caches, log messages.
   - Compare Rust and Python behaviour for:
     - `subscribe_trades` / `_handle_trade`.
     - `subscribe_book_deltas` / `_handle_orderbook`.
     - `subscribe_book_snapshots` / `_handle_orderbook_snapshot`.
     - `subscribe_quotes` / `_handle_deltas` + quote synthesis.
     - `subscribe_bars` / `_handle_kline`.
   - Identify any mismatches (e.g., missing snapshot bootstraps, depth handling) and plan minimal corrections in Rust.

2. **Crossed orderbook resolution parity**
   - Compare Python `_resolve_crossed_order_book` algorithm with Rust `resolve_crossed_order_book`:
     - Cases: bid > ask, bid < ask, bid == ask, multi‑iteration loops, partial removal.
     - RecordFlag handling (`F_LAST`) and sequence semantics.
   - Add or refine Rust tests to cover:
     - Same scenarios used in Python tests (where available).
     - Additional pathological cases (empty book, rapidly alternating crosses).
   - Confirm that final deltas emitted to `DataEvent::Data` are semantically equivalent to Python’s `OrderBookDeltas`.

3. **Quote generation parity**
   - Compare Python `_handle_deltas` quote generation with Rust `handle_deltas_message`:
     - Use of last quote when one side of the book is missing.
     - Conditions for emitting a new quote (only when any of bid/ask/size changes).
     - Handling of completely empty books (emit nothing vs reuse last quote with warning).
   - Add targeted Rust tests:
     - For various combinations of book updates (bid only, ask only, both, clearing one side).
     - For fallback to last quote when top‑of‑book is cleared.

4. **Bars (candles) parity**
   - Ensure `map_bar_spec_to_resolution` (doc‑level helper) and runtime mapping in `subscribe_bars`/`unsubscribe_bars`/`request_bars` match Python’s `get_interval_from_bar_type` / `_enum_parser.parse_dydx_kline`.
   - Confirm partitioning behaviour in Rust matches Python:
     - Range partitioning into multiple requests using bar duration × max_bars.
     - Limit application across chunks (stop once overall limit reached).
     - Incomplete bar filtering (`ts_event < now`).
   - Add Rust tests for:
     - Single‑request and multi‑request bar fetches.
     - Edge cases: start == end, open‑ended ranges, very large ranges with limit.

5. **Trade request parity**
   - Validate `request_trades` against Python `_request_trades` for:
     - Symbol mapping (`-PERP` suffix handling).
     - Limit semantics (defaults, large values).
     - Start/end timestamp filtering and ordering.
     - Handling of instruments not in cache (error vs empty result).
   - Extend Rust tests to mirror Python test cases documented in `DYDX_DATA_TESTING.md` (including error and edge cases).

6. **Instrument request parity**
   - Ensure `request_instruments`:
     - Populates instrument cache identically to Python’s instrument provider.
     - Returns the same instrument set (subject to any config filters, if introduced later).
   - Ensure `request_instrument`:
     - Prefers cache hit; on miss, fetches via HTTP and updates cache.
     - Emits responses with fully populated metadata (venue, timestamps, params) consistent with adapter guide and Python expectations.
   - Address test limitations:
     - Fix or refactor tests blocked by `get_data_event_sender` singleton, possibly by:
       - Injecting a test‑only sender into `DydxDataClient` constructor.
       - Or isolating tests at a lower level (HTTP clients + pure functions) where the singleton is not required.

### Phase 3 – Consistency with v4-client-rs & Indexer Spec

1. **HTTP endpoint alignment**
   - Cross‑reference `DydxRawHttpClient` methods with `v4-client-rs/client/src/indexer/rest`:
     - Markets, trades, candles, orderbook, accounts endpoints.
   - Verify:
     - Endpoint paths (`/v4/markets`, `/v4/trades`, `/v4/candles`, `/v4/orderbook`, etc.).
     - Query parameter names and combinations.
     - Pagination/limit semantics and default behaviours.
   - Plan corrections for any mismatches that affect data correctness.

2. **WebSocket channel alignment**
   - Compare `DydxWebSocketClient` subscription messages with `v4-client-rs/client/src/indexer/sock`:
     - Channel names (`v4_trades`, `v4_orderbook`, `v4_candles`, `v4_markets`).
     - Subscription/unsubscription payload formats.
   - Ensure reconnection/resubscription logic matches or improves on the official client.
   - Add Rust tests for WS message building where possible (unit‑level, not live network).

3. **Error handling & retry alignment**
   - Ensure HTTP error/retry policy in `DydxRawHttpClient` matches official spec and `should_retry_error_code` from `common::consts`.
   - Confirm that the data client handles HTTP and WS errors consistently with OKX/BitMEX patterns:
     - Use `anyhow::Context` for contextualized errors.
     - Emit empty responses on failure when appropriate, with clear logging.

### Phase 4 – Testing, Tooling & CI Integration

1. **Rust unit tests consolidation**
   - Keep existing tests in `dydx/src/data/mod.rs` but:
     - Ensure they compile and pass without interfering singletons.
     - Use `#[tokio::test]` where asynchronous behaviour is under test.
   - Add missing tests for:
     - `request_bars` (as outlined in `DYDX_DATA_TESTING.md`).
     - Cross resolution, quote generation, bar caching (explicit coverage).

2. **Python parity tests (read‑only)**
   - Treat existing Python tests under `tests/unit_tests/adapters/dydx` and `tests/integration_tests/adapters/dydx` strictly as a behavioural reference:
     - Derive expected behaviours and edge cases for the Rust implementation.
     - Do **not** modify or extend Python tests or Python code as part of this work.

3. **Integration tests (optional, networked)**
   - Plan optional testnet integration tests (can be gated behind feature flags):
     - Compare live data from testnet using Rust dYdX adapter against Python adapter for:
       - Instruments count and key fields.
       - Sample trades and bars (within tolerance).
   - Ensure these tests are opt‑in for CI to avoid flakiness.

4. **Tooling integration**
   - After implementing changes:
     - Run `make cargo-clippy` for linting and style checks.
     - Run `make pre-commit` (which should include format, lint, and tests) and confirm pass.
     - Run targeted tests for dYdX adapter:
       - `cargo test -p adapters-dydx`
       - Any workspace‑level tests that exercise `DataClient` flows.

### Phase 5 – Documentation & Developer Experience

1. **Update dYdX‑specific docs (Rust‑centric)**
   - Align `DYDX_DATA_PLAN.md` and `DYDX_DATA_TESTING.md` with actual implementation and test status after changes:
     - Mark completed items as done, add any new edge cases or subtleties discovered.
   - Add short section to `DYDX.md` or an appropriate existing document summarizing:
     - How to configure and use the dYdX data adapter from Rust (and via existing Python bindings where relevant, without changing Python code).
     - Known differences vs centralized venues (e.g., oracle prices, DEX nature).

2. **Adapter guide cross‑references**
   - Ensure `docs/developer_guide/adapters.md` can cite dYdX as a valid reference implementation (similar to OKX/BitMEX/Bybit) for:
     - Handling DEX‑style orderbooks.
     - Crossed orderbook resolution.
     - Partitioned historical bar requests.

3. **Developer ergonomics**
   - Confirm `crates/adapters/dydx/src/python` bindings expose:
     - `DydxHttpClient` and `DydxWebSocketClient` in a manner consistent with other adapters.
     - Data‑relevant helpers (instrument cache inspection, connection state).
   - Keep any new work scoped to Rust:
     - Add or adjust Rust data‑side helpers as needed.
     - Avoid changing Python bindings or Python code; rely on them only as consumers of the Rust API.

---

## 5. Execution Order Summary

1. **Align structure & style** to match adapter guide and OKX/BitMEX/Bybit.
2. **Tighten DataClient parity** with Python for:
   - Subscriptions/unsubscriptions.
   - Crossed book resolution and quote generation.
   - Historical trades and bars behaviour.
   - Instrument request behaviour and caching.
3. **Conform to v4-client-rs spec** for HTTP/WS endpoints and messages.
4. **Harden tests & tooling**:
   - Fix and extend Rust unit tests.
   - Use Python tests as behavioural oracle.
   - Integrate `cargo-clippy`, `pre-commit`, and adapter‑focused test runs.
5. **Document and finalize**:
   - Update dYdX plan/testing docs.
   - Ensure adapter guide can reference dYdX as a canonical Rust data adapter implementation.
