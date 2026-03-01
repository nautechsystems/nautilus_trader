# Kalshi Adapter Design

**Date:** 2026-03-01
**Status:** Approved
**Scope:** Data-only MVP (backtesting + live paper trading)

## Overview

A new NautilusTrader adapter for the Kalshi prediction market exchange. Kalshi is a regulated US prediction market where participants trade binary contracts on real-world events. The adapter targets API v2 (Kalshi's current production API, accessed at `/trade-api/v2`).

The adapter is data-only for the initial release — it provides instrument discovery, historical market data, and real-time market data feeds. Order execution is deferred to a future iteration.

## Architecture

The adapter follows the Polymarket adapter pattern: a pure Rust crate at `crates/adapters/kalshi/` with HTTP and WebSocket modules, exposed to Python via PyO3. A Python application layer in `nautilus_trader/adapters/kalshi/` provides the instrument provider, data client, and factory classes.

### Two Operating Modes

| Mode | Auth | Data Sources |
|------|------|-------------|
| Backtesting | None (public REST only) | Instrument discovery, historical trades, OHLCV candlesticks |
| Live paper trading | RSA-PSS (required) | Authenticated WebSocket orderbook deltas + trade stream |

## File Structure

```
crates/adapters/kalshi/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── config.rs                  # KalshiDataClientConfig
│   ├── common/
│   │   ├── mod.rs
│   │   ├── consts.rs              # Venue name, base URLs, rate limits, price bounds
│   │   ├── credential.rs          # RSA-PSS signing (KeyId + PEM private key)
│   │   ├── enums.rs               # KalshiMarketStatus, KalshiTakerSide, KalshiMarketType, CandlestickInterval
│   │   ├── models.rs              # Shared types
│   │   └── parse.rs               # Shared parsing utilities
│   ├── http/
│   │   ├── mod.rs
│   │   ├── client.rs              # KalshiHttpClient
│   │   ├── error.rs
│   │   ├── models.rs              # Series, Event, Market, Orderbook, Trade, Candlestick types
│   │   ├── parse.rs               # HTTP response → Nautilus types
│   │   └── rate_limits.rs         # 20 req/sec Basic tier (configurable)
│   ├── websocket/
│   │   ├── mod.rs
│   │   ├── client.rs              # KalshiWebSocketClient
│   │   ├── error.rs
│   │   ├── handler.rs             # Sequence tracking, snapshot/delta dispatch
│   │   └── messages.rs            # KalshiWsMessage enum (typed envelope)
│   └── python/
│       └── mod.rs                 # PyO3 module stub
├── tests/
│   ├── http.rs
│   └── websocket.rs
└── test_data/
    ├── http_markets.json
    ├── http_orderbook.json
    ├── http_trades.json
    ├── http_candlesticks.json
    ├── ws_orderbook_snapshot.json
    ├── ws_orderbook_delta.json
    └── ws_trade.json

nautilus_trader/adapters/kalshi/
├── __init__.py
├── config.py                      # KalshiDataClientConfig (Python dataclass)
├── providers.py                   # KalshiInstrumentProvider
├── data.py                        # KalshiDataClient
└── factories.py                   # KalshiLiveDataClientFactory, KalshiBacktestDataClientFactory
```

## Configuration

```rust
pub struct KalshiDataClientConfig {
    // Endpoints (default to production)
    pub base_url: String,           // "https://api.elections.kalshi.com/trade-api/v2"
    pub ws_url: String,             // "wss://api.elections.kalshi.com/trade-api/ws/v2"

    // Timeouts
    pub http_timeout_secs: u64,     // default: 60
    pub ws_timeout_secs: u64,       // default: 30

    // Instrument filtering (at least one of series_tickers or event_tickers required)
    pub series_tickers: Vec<String>,  // e.g. ["KXBTC", "PRES-2024"]
    pub event_tickers: Vec<String>,   // optional additional filter by event

    // Instrument reload interval
    pub instrument_reload_interval_mins: u64,  // default: 60

    // Rate limiting
    pub rate_limit_rps: u32,        // default: 20 (Basic tier)

    // Optional credentials (required for WebSocket / orderbook REST)
    pub api_key_id: Option<String>,      // env fallback: KALSHI_API_KEY_ID
    pub private_key_pem: Option<String>, // env fallback: KALSHI_PRIVATE_KEY_PEM
}
```

## Authentication

**Mechanism:** RSA-PSS with SHA-256 (MGF1-SHA256, salt = digest length = 32 bytes)

**`KalshiCredential`** (in `common/credential.rs`):
- Holds `api_key_id: String` and a parsed RSA private key
- `sign(&self, method: &str, path: &str) -> (timestamp_ms: String, signature_b64: String)`
- Message: `{timestamp_ms}{METHOD}{path}` — path stripped of query parameters
- Produces three headers: `KALSHI-ACCESS-KEY`, `KALSHI-ACCESS-TIMESTAMP`, `KALSHI-ACCESS-SIGNATURE`
- Zeroized on drop for security
- Rust crates: `rsa` + `sha2`

**Lazy auth**: HTTP and WebSocket clients accept `Option<KalshiCredential>`. Public REST calls proceed without credentials. Authenticated calls (orderbook snapshot, WebSocket upgrade) fail fast with a descriptive error if credentials are absent.

## HTTP Client

**`KalshiHttpClient`** wraps `nautilus-network`'s `HttpClient`.

### Instrument Discovery (public, no auth)

```rust
async fn get_series(&self, tickers: &[String]) -> Result<Vec<KalshiSeries>>
async fn get_events(&self, series_tickers: &[String]) -> Result<Vec<KalshiEvent>>
async fn get_markets(&self, event_tickers: &[String], series_tickers: &[String]) -> Result<Vec<KalshiMarket>>
```

### Historical Data (public, no auth)

```rust
async fn get_trades(
    &self,
    market_ticker: &str,
    min_ts: Option<u64>,
    max_ts: Option<u64>,
    cursor: Option<&str>,
) -> Result<(Vec<KalshiTrade>, Option<String>)>   // returns (trades, next_cursor)

async fn get_candlesticks(
    &self,
    market_ticker: &str,
    start_ts: u64,
    end_ts: u64,
    period_interval: CandlestickInterval,  // Minutes1 = 1, Hours1 = 60, Days1 = 1440
) -> Result<Vec<KalshiCandlestick>>
```

Endpoint: `GET /historical/markets/{ticker}/candlesticks`

### Live Snapshot (authenticated, for paper trading WS init)

```rust
async fn get_orderbook(&self, market_ticker: &str, depth: Option<u32>) -> Result<KalshiOrderbook>
```

### Pagination

`get_trades` and `get_markets` use cursor-based pagination. The client handles multi-page fetches, returning a `next_cursor` for the caller to continue fetching.

## Instrument Provider (Python Layer)

`KalshiInstrumentProvider`:
1. On startup: calls `get_series` → `get_events` → `get_markets` filtered by config
2. Each `KalshiMarket` maps to one `BinaryOption` instrument (YES side as primary)
3. Field mapping:
   - `ticker` → `InstrumentId`
   - `close_time` → `expiration_ns`
   - Currency: USD
   - Price precision: 4 decimal places (to support subpenny pricing)
   - Size precision: 2 decimal places
   - `maker_fee` / `taker_fee` from series `fee_multiplier`
4. Reloads on `instrument_reload_interval_mins`

## WebSocket Client (Live Paper Trading)

**`KalshiWebSocketClient`** uses `nautilus-network`'s `WebSocketClient` with a single authenticated connection.

### Subscription API

```rust
async fn subscribe_orderbook(&self, market_tickers: &[String]) -> Result<()>
async fn subscribe_trades(&self, market_tickers: &[String]) -> Result<()>
async fn unsubscribe(&self, sid: u32) -> Result<()>
```

Subscription command format:
```json
{
  "id": 1,
  "cmd": "subscribe",
  "params": {
    "channels": ["orderbook_delta"],
    "market_tickers": ["KXBTC-25MAR15-B100000"]
  }
}
```

### Message Envelope

```rust
enum KalshiWsMessage {
    OrderbookSnapshot(KalshiOrderbookSnapshot),  // type: "orderbook_snapshot"
    OrderbookDelta(KalshiOrderbookDelta),        // type: "orderbook_delta"
    Trade(KalshiTradeEvent),                     // type: "trade"
    Error(KalshiWsError),                        // type: "error"
}
```

### Sequence Tracking

- Per-subscription map of `sid → last_seq`
- On delta: if `seq != last_seq + 1`, log warning and re-subscribe for fresh snapshot
- On snapshot: reset `last_seq = seq`, apply any buffered deltas with `seq > snapshot.seq`

### Orderbook Reconstruction

- Snapshot: YES bids populate directly; NO bids converted to YES asks via `1.00 - no_price`
- Deltas: update individual price levels; `delta == 0` removes a level
- Result emitted as `OrderBookDeltas` to the Nautilus data engine

### Authentication

RSA-PSS headers passed as HTTP upgrade headers during WebSocket handshake, signing `GET /trade-api/ws/v2`. Absent credentials produce a descriptive error.

### Keep-alive

Kalshi sends WebSocket Ping frames every 10 seconds. `nautilus-network` handles Pong responses automatically.

## Key Data Models

### Pricing

- All prices use `_dollars` / `_fp` fields exclusively (legacy integer cent fields deprecated March 5, 2026)
- Dollar strings with up to 4 decimal places: `"0.4200"`, `"0.0025"` (subpenny)
- Price range: `0.0001` to `0.9999`
- Settlement: each binary contract settles at $1.00 (YES) or $0.00 (NO)

### YES/NO Duality

Every Kalshi market has two complementary binary outcomes:
- `YES price + NO price = $1.00`
- YES bid at X → implied YES ask from NO side at `1.00 - NO_best_bid`
- Orderbook only exposes bids; asks are derived

### Candlestick (`KalshiCandlestick`)

```rust
pub struct KalshiCandlestick {
    pub end_period_ts: u64,
    pub yes_bid: KalshiOhlc,    // open/high/low/close dollar strings
    pub yes_ask: KalshiOhlc,
    pub price: KalshiPriceOhlc, // trade price open/high/low/close/mean
    pub volume: String,          // contract count, 2 decimal places
    pub open_interest: String,
}
```

Bar mapping: `price.open/high/low/close` → `Bar.open/high/low/close`, `volume` → `Bar.volume`.

## Integration

### Workspace (`Cargo.toml`)

```toml
nautilus-kalshi = { path = "crates/adapters/kalshi", version = "0.54.0", default-features = false }
```

### PyO3 Registration (`crates/pyo3/src/lib.rs`)

```rust
let n = "kalshi";
let submodule = pyo3::wrap_pymodule!(nautilus_kalshi::python::kalshi);
m.add_wrapped(submodule)?;
sys_modules.set_item(format!("{module_name}.{n}"), m.getattr(n)?)?;
```

## Testing Strategy

- **Unit tests** (`tests/http.rs`, `tests/websocket.rs`): parse `test_data/` JSON fixtures, no network calls
- **Fixtures**: sourced from real Kalshi API responses for each message type
- **Integration tests** (optional, feature-flagged): hit the demo environment (`demo-api.kalshi.co`)
- **Python tests**: `tests/unit_tests/adapters/kalshi/` following the Polymarket test structure

## API Reference

- Docs: https://docs.kalshi.com
- REST base: `https://api.elections.kalshi.com/trade-api/v2`
- WS base: `wss://api.elections.kalshi.com/trade-api/ws/v2`
- Demo REST: `https://demo-api.kalshi.co/trade-api/v2`
- Demo WS: `wss://demo-api.kalshi.co/trade-api/ws/v2`
- Candlesticks: `GET /historical/markets/{ticker}/candlesticks`
- Rate limits: 20 req/sec (Basic), 30 (Advanced), 100 (Premier), 400 (Prime)
