# Adapters

## Introduction

This developer guide provides specifications and instructions on how to develop an integration adapter for the NautilusTrader platform.
Adapters provide connectivity to trading venues and data providers—translating raw venue APIs into Nautilus’s unified interface and normalized domain model.

## Structure of an adapter

NautilusTrader adapters follow a layered architecture pattern with:

- **Rust core** for networking clients and performance-critical operations.
- **Python layer** (optional) for integrating into the legacy system.

Good references for consistent patterns are currently:

- BitMEX
- Bybit
- OKX

### Rust core (`crates/adapters/your_adapter/`)

The Rust layer handles:

- **HTTP client**: Raw API communication, request signing, rate limiting.
- **WebSocket client**: Low-latency streaming connections, message parsing.
- **Parsing**: Fast conversion of venue data to Nautilus domain models.
- **Python bindings**: PyO3 exports to make Rust functionality available to Python.

Typical Rust structure:

```
crates/adapters/your_adapter/
├── src/
│   ├── common/           # Shared types and utilities
│   │   ├── consts.rs     # Venue constants / broker IDs
│   │   ├── credential.rs # API key storage and signing helpers
│   │   ├── enums.rs      # Venue enums mirrored in REST/WS payloads
│   │   ├── urls.rs       # Environment & product aware base-url resolvers
│   │   ├── parse.rs      # Shared parsing helpers
│   │   └── testing.rs    # Fixtures reused across unit tests
│   ├── http/             # HTTP client implementation
│   │   ├── client.rs     # HTTP client with authentication
│   │   ├── models.rs     # Structs for REST payloads
│   │   ├── query.rs      # Request and query builders
│   │   └── parse.rs      # Response parsing functions
│   ├── websocket/        # WebSocket implementation
│   │   ├── client.rs     # WebSocket client
│   │   ├── messages.rs   # Structs for stream payloads
│   │   └── parse.rs      # Message parsing functions
│   ├── python/           # PyO3 Python bindings
│   ├── config.rs         # Configuration structures
│   └── lib.rs            # Library entry point
└── tests/                # Integration tests with mock servers
```

### Python layer (`nautilus_trader/adapters/your_adapter`)

The Python layer provides the integration interface through these components:

1. **Instrument Provider**: Supplies instrument definitions via `InstrumentProvider`.
2. **Data Client**: Handles market data feeds and historical data requests via `LiveDataClient` and `LiveMarketDataClient`.
3. **Execution Client**: Manages order execution via `LiveExecutionClient`.
4. **Factories**: Converts venue-specific data to Nautilus domain models.
5. **Configuration**: User-facing configuration classes for client settings.

Typical Python structure:

```
nautilus_trader/adapters/your_adapter/
├── config.py     # Configuration classes
├── constants.py  # Adapter constants
├── data.py       # LiveDataClient/LiveMarketDataClient
├── execution.py  # LiveExecutionClient
├── factories.py  # Instrument factories
├── providers.py  # InstrumentProvider
└── __init__.py   # Package initialization
```

## Adapter implementation steps

1. Create a new Python subpackage for your adapter.
2. Implement the Instrument Provider by inheriting from `InstrumentProvider` and implementing the necessary methods to load instruments.
3. Implement the Data Client by inheriting from either the `LiveDataClient` or `LiveMarketDataClient` class as applicable, providing implementations for the required methods.
4. Implement the Execution Client by inheriting from `LiveExecutionClient` and providing implementations for the required methods.
5. Create configuration classes to hold your adapter’s settings.
6. Test your adapter thoroughly to ensure all methods are correctly implemented and the adapter works as expected (see the [Testing Guide](testing.md)).

---

## Rust adapter patterns

- **Common code (`common/`)**: Group venue constants, credential helpers, enums, and reusable parsers under `src/common`. Adapters such as OKX keep submodules like `consts`, `credential`, `enums`, and `urls` alongside a `testing` module for fixtures, providing a single place for cross-cutting pieces. When an adapter has multiple environments or product categories, add a dedicated `common::urls` helper so REST/WebSocket base URLs stay in sync with the Python layer.
- **Configurations (`config.rs`)**: Expose typed config structs in `src/config.rs` so Python callers toggle venue-specific behaviour (see how OKX wires demo URLs, retries, and channel flags). Keep defaults minimal and delegate URL selection to helpers in `common::urls`.
- **Error taxonomy (`error.rs`)**: Centralise HTTP/WebSocket failure handling in an adapter-specific error enum. BitMEX, for example, separates retryable, non-retryable, and fatal variants while embedding the original transport error—follow that shape so operational tooling can react consistently.
- **Python exports (`python/mod.rs`)**: Mirror the Rust surface area through PyO3 modules by re-exporting clients, enums, and helper functions. When new functionality lands in Rust, add it to `python/mod.rs` so the Python layer stays in sync (the OKX adapter is a good reference).
- **Python bindings (`python/`)**: Expose Rust functionality to Python through PyO3. Mark venue-specific structs that need Python access with `#[pyclass]` and implement `#[pymethods]` blocks with `#[getter]` attributes for field access. For async methods in the HTTP client, use `pyo3_async_runtimes::tokio::future_into_py` to convert Rust futures into Python awaitables. When returning lists of custom types, map each item with `Py::new(py, item)` before constructing the Python list. Register all exported classes and enums in `python/mod.rs` using `m.add_class::<YourType>()` so they're available to Python code. Follow the pattern established in other adapters: prefixing Python-facing methods with `py_*` in Rust while using `#[pyo3(name = "method_name")]` to expose them without the prefix.
- **String interning**: Use `ustr::Ustr` for any non-unique strings the platform stores repeatedly (venues, symbols, instrument IDs) to minimise allocations and comparisons.
- **Testing helpers (`common/testing.rs`)**: Store shared fixtures and payload loaders in `src/common/testing.rs` for use across HTTP and WebSocket unit tests. This keeps `#[cfg(test)]` helpers out of production modules and encourages reuse.

## HTTP client patterns

Adapters use a two-layer HTTP client structure to enable efficient cloning for Python bindings while keeping the actual HTTP logic in a single place.

### Client structure

Use an inner/outer client pattern with `Arc` wrapping:

```rust
use std::sync::Arc;
use nautilus_network::http::HttpClient;

// Inner client - contains actual HTTP logic
pub struct MyHttpInnerClient {
    base_url: String,
    client: HttpClient,  // Use nautilus_network::http::HttpClient, not reqwest directly
    credential: Option<Credential>,
    retry_manager: RetryManager<MyHttpError>,
    cancellation_token: CancellationToken,
}

// Outer client - wraps inner with Arc for cheap cloning (needed for Python)
pub struct MyHttpClient {
    pub(crate) inner: Arc<MyHttpInnerClient>,
}
```

**Key points**:

- Inner client (`*HttpInnerClient`) contains all HTTP logic and state.
- Outer client (`*HttpClient`) wraps the inner client in an `Arc` for efficient cloning.
- Use `nautilus_network::http::HttpClient` instead of `reqwest::Client` directly - this provides rate limiting, retry logic, and consistent error handling.
- The outer client delegates all methods to the inner client.

### Parser functions

Parser functions convert venue-specific data structures into Nautilus domain objects. These belong in `common/parse.rs` for cross-cutting conversions (instruments, trades, bars) or `http/parse.rs` for REST-specific transformations. Each parser takes venue data plus context (account IDs, timestamps, instrument references) and returns a Nautilus domain type wrapped in `Result`.

**Standard patterns:**

- Handle string-to-numeric conversions with proper error context using `.parse::<f64>()` and `anyhow::Context`.
- Check for empty strings before parsing optional fields - venues often return `""` instead of omitting fields.
- Map venue enums to Nautilus enums explicitly with `match` statements rather than implementing automatic conversions that could hide mapping errors.
- Accept instrument references when precision or other metadata is required for constructing Nautilus types (quantities, prices).
- Use descriptive function names: `parse_position_status_report`, `parse_order_status_report`, `parse_trade_tick`.

Place parsing helpers (`parse_price_with_precision`, `parse_timestamp`) in the same module as private functions when they're reused across multiple parsers.

### Method naming and organization

Organize HTTP methods into two distinct sections:

- Low-level direct API calls.
- High-level domain methods.

**Naming conventions:**

- **Low-level API methods**: Prefix with `http_` and place near the top of the impl block (e.g., `http_get_instruments`, `http_place_order`).
- **High-level domain methods**: No prefix, placed in a separate section (e.g., `submit_order`, `cancel_order`).
- Low-level methods take venue-specific types (builders, JSON values).
- High-level methods take Nautilus domain objects (InstrumentId, ClientOrderId, OrderSide, etc.).

**High-level method flow:**

High-level domain methods in the inner client follow a three-step pattern: build venue-specific parameters from Nautilus types, call the corresponding `http_*` method, then parse or extract the response. For endpoints returning domain objects (positions, orders, trades), call parser functions from `common/parse`. For endpoints returning raw venue data (fee rates, balances), extract the result directly from the response envelope. Methods prefixed with `request_*` indicate they return domain data, while methods like `submit_*`, `cancel_*`, or `modify_*` perform actions and return acknowledgments.

The outer client delegates all methods directly to the inner client without additional logic - this separation exists solely to enable cheap cloning for Python bindings via `Arc`.

### Query parameter builders

Use the `derive_builder` crate with proper defaults and ergonomic Option handling:

```rust
use derive_builder::Builder;

#[derive(Clone, Debug, Deserialize, Serialize, Builder)]
#[serde(rename_all = "camelCase")]
#[builder(setter(into, strip_option), default)]
pub struct InstrumentsInfoParams {
    pub category: ProductType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl Default for InstrumentsInfoParams {
    fn default() -> Self {
        Self {
            category: ProductType::Linear,
            symbol: None,
            limit: None,
        }
    }
}
```

**Key attributes:**

- `#[builder(setter(into, strip_option), default)]` - enables clean API: `.symbol("BTCUSDT")` instead of `.symbol(Some("BTCUSDT".to_string()))`.
- `#[serde(skip_serializing_if = "Option::is_none")]` - omits optional fields from query strings.
- Always implement `Default` for builder parameters.

### Request signing and authentication

Keep signing logic in the inner client.

### Error handling and retry logic

Use the `RetryManager` from `nautilus_network` for consistent retry behavior.

### Rate limiting

Configure rate limiting through `HttpClient`.

## WebSocket client patterns

WebSocket clients handle real-time streaming data and require careful management of connection state, authentication, subscriptions, and reconnection logic.

### Client structure

WebSocket clients typically don't need the inner/outer pattern since they're not frequently cloned. Use a single struct with clear state management.

### Authentication

Handle authentication separately from subscriptions.

### Subscription management

#### Subscription lifecycle

A **subscription** represents any topic in one of two states:

| State         | Description |
|---------------|-------------|
| **Pending**   | Subscription request sent to venue, awaiting acknowledgment |
| **Confirmed** | Venue acknowledged subscription and is actively streaming data |

State transitions follow this lifecycle:

| Trigger           | Method Called        | From State | To State  | Notes |
|-------------------|----------------------|------------|-----------|-------|
| User subscribes   | `mark_subscribe()`   | —          | Pending   | Topic added to pending set |
| Venue confirms    | `confirm()`          | Pending    | Confirmed | Moved from pending to confirmed |
| Venue rejects     | `mark_failure()`     | Pending    | Pending   | Stays pending for retry on reconnect |
| User unsubscribes | `mark_unsubscribe()` | Confirmed  | Pending   | Temporarily pending until ack |
| Unsubscribe ack   | `clear_pending()`    | Pending    | Removed   | Topic fully removed |

**Key principles**:

- `subscription_count()` reports **only confirmed subscriptions**, not pending ones.
- Failed subscriptions remain pending and are automatically retried on reconnect.
- Both confirmed and pending subscriptions are restored after reconnection.
- Unsubscribe operations must check the `op` field in acknowledgments to avoid re-confirming topics.

#### Topic format patterns

Adapters use venue-specific delimiters to structure subscription topics:

| Adapter    | Delimiter | Example                | Pattern                      |
|------------|-----------|------------------------|------------------------------|
| **BitMEX** | `:`       | `trade:XBTUSD`         | `{channel}:{symbol}`         |
| **OKX**    | `:`       | `trades:BTC-USDT-SWAP` | `{channel}:{symbol}`         |
| **Bybit**  | `.`       | `orderbook.50.BTCUSDT` | `{channel}.{depth}.{symbol}` |

Parse topics using `split_once()` with the appropriate delimiter to extract channel and symbol components.

### Reconnection logic

On reconnection, restore authentication and public subscriptions, but skip private channels that were explicitly unsubscribed.

### Ping/Pong handling

Support both control frame pings and application-level pings.

### Message routing

Route different message types to appropriate handlers.

### Error handling

Classify errors to determine retry behavior:

```rust
#[derive(Debug, thiserror::Error)]
pub enum WebSocketError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("Subscription failed: {0}")]
    SubscriptionFailed(String),

    #[error("Message parse error: {0}")]
    ParseError(String),
}

impl MyWebSocketClient {
    fn should_reconnect(&self, error: &WebSocketError) -> bool {
        matches!(
            error,
            WebSocketError::ConnectionFailed(_)
        )
    }
}
```

## Modeling venue payloads

Use the following conventions when mirroring upstream schemas in Rust.

### REST models (`http::models` and `http::query`)

- Put request and response representations in `src/http/models.rs` and derive `serde::Deserialize` (add `serde::Serialize` when the adapter sends data back).
- Mirror upstream payload names with blanket casing attributes such as `#[serde(rename_all = "camelCase")]` or `#[serde(rename_all = "snake_case")]`; only add per-field renames when the upstream key would be an invalid Rust identifier or collide with a keyword (for example `#[serde(rename = "type")] pub order_type: String`).
- Keep helper structs for query parameters in `src/http/query.rs`, deriving `serde::Serialize` to remain type-safe and reusing constants from `common::consts` instead of duplicating literals.

### WebSocket messages (`websocket::messages`)

- Define streaming payload types in `src/websocket/messages.rs`, giving each venue topic a struct or enum that mirrors the upstream JSON.
- Apply the same naming guidance as REST models: rely on blanket casing renames and keep field names aligned with the venue unless syntax forces a change; consider serde helpers such as `#[serde(tag = "op")]` or `#[serde(flatten)]` and document the choice.
- Note any intentional deviations from the upstream schema in code comments and module docs so other contributors can follow the mapping quickly.

---

## Testing

Adapters should ship two layers of coverage: the Rust crate that talks to the venue and the Python glue that exposes it to the wider platform.
Keep the suites deterministic and colocated with the production code they protect.

### Rust testing

#### Layout

```
crates/adapters/your_adapter/
├── src/
│   ├── http/
│   │   ├── client.rs      # HTTP client + unit tests
│   │   └── parse.rs       # REST payload parsers + unit tests
│   └── websocket/
│       ├── client.rs      # WebSocket client + unit tests
│       └── parse.rs       # Streaming parsers + unit tests
├── tests/
│   ├── http.rs            # Mock HTTP integration tests
│   └── websocket.rs       # Mock WebSocket integration tests
└── test_data/             # Canonical venue payloads used by the suites
    ├── http_get_{endpoint}.json  # Full venue responses with retCode/result/time
    └── ws_{message_type}.json    # WebSocket message samples
```

- Place unit tests next to the module they exercise (`#[cfg(test)]` blocks). Use `src/common/testing.rs` (or an equivalent helper module) for shared fixtures so production files stay tidy.
- Keep Axum-based integration suites under `crates/adapters/<adapter>/tests/`, mirroring the public APIs (HTTP client, WebSocket client, caches, reconnection flows).
- Store upstream payload samples (snapshots, REST replies) under `test_data/` and reference them from both unit and integration tests. Name test data files consistently: `http_get_{endpoint_name}.json` for REST responses, `ws_{message_type}.json` for WebSocket messages. Include complete venue response envelopes (status codes, timestamps, result wrappers) rather than just the data payload. Provide multiple realistic examples in each file - for instance, position data should include long, short, and flat positions to exercise all parser branches.

#### Unit tests

- Focus on pure logic: parsers, signing helpers, canonicalisers, and any business rules that do not require a live transport.
- Avoid duplicating coverage that the integration tests already provide.

#### Integration tests

Exercise the public API against Axum mock servers. At a minimum, mirror the BitMEX test surface (see
`crates/adapters/bitmex/tests/`) so every adapter proves the same behaviours.

##### HTTP client integration coverage

- **Happy paths** – fetch a representative public resource (e.g., instruments or mark price) and ensure the
  response is converted into Nautilus domain models.
- **Credential guard** – call a private endpoint without credentials and assert a structured error; repeat with
  credentials to prove success.
- **Rate limiting / retry mapping** – surface venue-specific rate-limit responses and assert the adapter produces
  the correct `OkxError`/`BitmexHttpError` variant so the retry policy can react.
- **Query builders** – exercise builders for paginated/time-bounded endpoints (historical trades, candles) and
  assert the emitted query string matches the venue specification (`after`, `before`, `limit`, etc.).
- **Error translation** – verify non-2xx upstream responses map to adapter error enums with the original code/message attached.

##### WebSocket client integration coverage

- **Login handshake** – confirm a successful login flips the internal auth state and test failure cases where the
  server returns a non-zero code; the client should surface an error and avoid marking itself as authenticated.
- **Ping/Pong** – prove both text-based and control-frame pings trigger immediate pong responses.
- **Subscription lifecycle** – assert subscription requests/acks are emitted for public and private channels, and that
  unsubscribe calls remove entries from the cached subscription sets.
- **Reconnect behaviour** – simulate a disconnect and ensure the client re-authenticates, restores public channels,
  and skips private channels that were explicitly unsubscribed pre-disconnect.
- **Message routing** – feed representative data/ack/error payloads through the socket and assert they arrive on the
  public stream as the correct `NautilusWsMessage` variant.
- **Quota tagging** – (optional but recommended) validate that order/cancel/amend operations are tagged with the
  appropriate quota label so rate limiting can be enforced independently of subscription traffic.

- Prefer event-driven assertions with shared state (for example, collect `subscription_events`, track
  pending/confirmed topics, wait for `connection_count` transitions) instead of arbitrary `sleep` calls.
- Use adapter-specific helpers to gate on explicit signals such as "auth confirmed" or "reconnection finished" so
  suites remain deterministic under load.

### Python testing

- Exercise the adapter’s Python surface (instrument providers, data/execution clients, factories) inside `tests/integration_tests/adapters/<adapter>/`.
- Mock the PyO3 boundary (`nautilus_pyo3` shims, stubbed Rust clients) so tests stay fast while verifying that configuration, factory wiring, and error handling match the exported Rust API.
- Mirror the Rust integration coverage: when the Rust suite adds a new behaviour (e.g., reconnection replay, error
  propagation), assert the Python layer performs the same sequence (connect/disconnect, submit/amend/cancel
  translations, venue ID hand-off, failure handling). BitMEX’s Python tests provide the target level of detail.

---

## Python adapter layer

Below is a step-by-step guide to building an adapter for a new data provider using the provided template.

### InstrumentProvider

The `InstrumentProvider` supplies instrument definitions available on the venue. This
includes loading all available instruments, specific instruments by ID, and applying filters to the
instrument list.

```python
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.model import InstrumentId


class TemplateInstrumentProvider(InstrumentProvider):
    """Example `InstrumentProvider` showing the minimal overrides required for a complete integration."""

    async def load_all_async(self, filters: dict | None = None) -> None:
        raise NotImplementedError("implement `load_all_async` in your adapter subclass")

    async def load_ids_async(self, instrument_ids: list[InstrumentId], filters: dict | None = None) -> None:
        raise NotImplementedError("implement `load_ids_async` in your adapter subclass")

    async def load_async(self, instrument_id: InstrumentId, filters: dict | None = None) -> None:
        raise NotImplementedError("implement `load_async` in your adapter subclass")
```

**Key methods**:

- `load_all_async`: Loads all instruments asynchronously, optionally applying filters.
- `load_ids_async`: Loads specific instruments by their IDs.
- `load_async`: Loads a single instrument by its ID.

### DataClient

The `LiveDataClient` handles the subscription and management of data feeds that are not specifically
related to market data. This might include news feeds, custom data streams, or other data sources
that enhance trading strategies but do not directly represent market activity.

```python
from nautilus_trader.data.messages import RequestData
from nautilus_trader.data.messages import SubscribeData
from nautilus_trader.data.messages import UnsubscribeData
from nautilus_trader.live.data_client import LiveDataClient
from nautilus_trader.model import DataType


class TemplateLiveDataClient(LiveDataClient):
    """Example `LiveDataClient` showing the overridable abstract methods."""

    async def _connect(self) -> None:
        raise NotImplementedError("implement `_connect` in your adapter subclass")

    async def _disconnect(self) -> None:
        raise NotImplementedError("implement `_disconnect` in your adapter subclass")

    async def _subscribe(self, command: SubscribeData) -> None:
        raise NotImplementedError("implement `_subscribe` in your adapter subclass")

    async def _unsubscribe(self, command: UnsubscribeData) -> None:
        raise NotImplementedError("implement `_unsubscribe` in your adapter subclass")

    async def _request(self, request: RequestData) -> None:
        raise NotImplementedError("implement `_request` in your adapter subclass")
```

**Key methods**:

- `_connect`: Establishes a connection to the data provider.
- `_disconnect`: Closes the connection to the data provider.
- `_subscribe`: Subscribes to a specific data type.
- `_unsubscribe`: Unsubscribes from a specific data type.
- `_request`: Requests data from the provider.

### MarketDataClient

The `MarketDataClient` handles market-specific data such as order books, top-of-book quotes and trades,
and instrument status updates. It focuses on providing historical and real-time market data that is essential for
trading operations.

```python
from nautilus_trader.data.messages import RequestBars
from nautilus_trader.data.messages import RequestData
from nautilus_trader.data.messages import RequestInstrument
from nautilus_trader.data.messages import RequestInstruments
from nautilus_trader.data.messages import RequestOrderBookSnapshot
from nautilus_trader.data.messages import RequestQuoteTicks
from nautilus_trader.data.messages import RequestTradeTicks
from nautilus_trader.data.messages import SubscribeBars
from nautilus_trader.data.messages import SubscribeData
from nautilus_trader.data.messages import SubscribeFundingRates
from nautilus_trader.data.messages import SubscribeIndexPrices
from nautilus_trader.data.messages import SubscribeInstrument
from nautilus_trader.data.messages import SubscribeInstrumentClose
from nautilus_trader.data.messages import SubscribeInstruments
from nautilus_trader.data.messages import SubscribeInstrumentStatus
from nautilus_trader.data.messages import SubscribeMarkPrices
from nautilus_trader.data.messages import SubscribeOrderBook
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.data.messages import UnsubscribeBars
from nautilus_trader.data.messages import UnsubscribeData
from nautilus_trader.data.messages import UnsubscribeFundingRates
from nautilus_trader.data.messages import UnsubscribeIndexPrices
from nautilus_trader.data.messages import UnsubscribeInstrument
from nautilus_trader.data.messages import UnsubscribeInstrumentClose
from nautilus_trader.data.messages import UnsubscribeInstruments
from nautilus_trader.data.messages import UnsubscribeInstrumentStatus
from nautilus_trader.data.messages import UnsubscribeMarkPrices
from nautilus_trader.data.messages import UnsubscribeOrderBook
from nautilus_trader.data.messages import UnsubscribeQuoteTicks
from nautilus_trader.data.messages import UnsubscribeTradeTicks
from nautilus_trader.live.data_client import LiveMarketDataClient


class TemplateLiveMarketDataClient(LiveMarketDataClient):
    """Example `LiveMarketDataClient` showing the overridable abstract methods."""

    async def _connect(self) -> None:
        raise NotImplementedError("implement `_connect` in your adapter subclass")

    async def _disconnect(self) -> None:
        raise NotImplementedError("implement `_disconnect` in your adapter subclass")

    async def _subscribe(self, command: SubscribeData) -> None:
        raise NotImplementedError("implement `_subscribe` in your adapter subclass")

    async def _unsubscribe(self, command: UnsubscribeData) -> None:
        raise NotImplementedError("implement `_unsubscribe` in your adapter subclass")

    async def _request(self, request: RequestData) -> None:
        raise NotImplementedError("implement `_request` in your adapter subclass")

    async def _subscribe_instruments(self, command: SubscribeInstruments) -> None:
        raise NotImplementedError("implement `_subscribe_instruments` in your adapter subclass")

    async def _unsubscribe_instruments(self, command: UnsubscribeInstruments) -> None:
        raise NotImplementedError("implement `_unsubscribe_instruments` in your adapter subclass")

    async def _subscribe_instrument(self, command: SubscribeInstrument) -> None:
        raise NotImplementedError("implement `_subscribe_instrument` in your adapter subclass")

    async def _unsubscribe_instrument(self, command: UnsubscribeInstrument) -> None:
        raise NotImplementedError("implement `_unsubscribe_instrument` in your adapter subclass")

    async def _subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        raise NotImplementedError("implement `_subscribe_order_book_deltas` in your adapter subclass")

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        raise NotImplementedError("implement `_unsubscribe_order_book_deltas` in your adapter subclass")

    async def _subscribe_order_book_snapshots(self, command: SubscribeOrderBook) -> None:
        raise NotImplementedError("implement `_subscribe_order_book_snapshots` in your adapter subclass")

    async def _unsubscribe_order_book_snapshots(self, command: UnsubscribeOrderBook) -> None:
        raise NotImplementedError("implement `_unsubscribe_order_book_snapshots` in your adapter subclass")

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        raise NotImplementedError("implement `_subscribe_quote_ticks` in your adapter subclass")

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        raise NotImplementedError("implement `_unsubscribe_quote_ticks` in your adapter subclass")

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        raise NotImplementedError("implement `_subscribe_trade_ticks` in your adapter subclass")

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        raise NotImplementedError("implement `_unsubscribe_trade_ticks` in your adapter subclass")

    async def _subscribe_mark_prices(self, command: SubscribeMarkPrices) -> None:
        raise NotImplementedError("implement `_subscribe_mark_prices` in your adapter subclass")

    async def _unsubscribe_mark_prices(self, command: UnsubscribeMarkPrices) -> None:
        raise NotImplementedError("implement `_unsubscribe_mark_prices` in your adapter subclass")

    async def _subscribe_index_prices(self, command: SubscribeIndexPrices) -> None:
        raise NotImplementedError("implement `_subscribe_index_prices` in your adapter subclass")

    async def _unsubscribe_index_prices(self, command: UnsubscribeIndexPrices) -> None:
        raise NotImplementedError("implement `_unsubscribe_index_prices` in your adapter subclass")

    async def _subscribe_funding_rates(self, command: SubscribeFundingRates) -> None:
        raise NotImplementedError("implement `_subscribe_funding_rates` in your adapter subclass")

    async def _unsubscribe_funding_rates(self, command: UnsubscribeFundingRates) -> None:
        raise NotImplementedError("implement `_unsubscribe_funding_rates` in your adapter subclass")

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        raise NotImplementedError("implement `_subscribe_bars` in your adapter subclass")

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        raise NotImplementedError("implement `_unsubscribe_bars` in your adapter subclass")

    async def _subscribe_instrument_status(self, command: SubscribeInstrumentStatus) -> None:
        raise NotImplementedError("implement `_subscribe_instrument_status` in your adapter subclass")

    async def _unsubscribe_instrument_status(self, command: UnsubscribeInstrumentStatus) -> None:
        raise NotImplementedError("implement `_unsubscribe_instrument_status` in your adapter subclass")

    async def _subscribe_instrument_close(self, command: SubscribeInstrumentClose) -> None:
        raise NotImplementedError("implement `_subscribe_instrument_close` in your adapter subclass")

    async def _unsubscribe_instrument_close(self, command: UnsubscribeInstrumentClose) -> None:
        raise NotImplementedError("implement `_unsubscribe_instrument_close` in your adapter subclass")

    async def _request_instrument(self, request: RequestInstrument) -> None:
        raise NotImplementedError("implement `_request_instrument` in your adapter subclass")

    async def _request_instruments(self, request: RequestInstruments) -> None:
        raise NotImplementedError("implement `_request_instruments` in your adapter subclass")

    async def _request_quote_ticks(self, request: RequestQuoteTicks) -> None:
        raise NotImplementedError("implement `_request_quote_ticks` in your adapter subclass")

    async def _request_trade_ticks(self, request: RequestTradeTicks) -> None:
        raise NotImplementedError("implement `_request_trade_ticks` in your adapter subclass")

    async def _request_bars(self, request: RequestBars) -> None:
        raise NotImplementedError("implement `_request_bars` in your adapter subclass")

    async def _request_order_book_snapshot(self, request: RequestOrderBookSnapshot) -> None:
        raise NotImplementedError("implement `_request_order_book_snapshot` in your adapter subclass")
```

**Key methods**:

- `_connect`: Establishes a connection to the venue APIs.
- `_disconnect`: Closes the connection to the venue APIs.
- `_subscribe`: Subscribes to generic data (base method for custom data types).
- `_unsubscribe`: Unsubscribes from generic data (base method for custom data types).
- `_request`: Requests generic data (base method for custom data types).
- `_subscribe_instruments`: Subscribes to market data for multiple instruments.
- `_unsubscribe_instruments`: Unsubscribes from market data for multiple instruments.
- `_subscribe_instrument`: Subscribes to market data for a single instrument.
- `_unsubscribe_instrument`: Unsubscribes from market data for a single instrument.
- `_subscribe_order_book_deltas`: Subscribes to order book delta updates.
- `_unsubscribe_order_book_deltas`: Unsubscribes from order book delta updates.
- `_subscribe_order_book_snapshots`: Subscribes to order book snapshot updates.
- `_unsubscribe_order_book_snapshots`: Unsubscribes from order book snapshot updates.
- `_subscribe_quote_ticks`: Subscribes to top-of-book quote updates.
- `_unsubscribe_quote_ticks`: Unsubscribes from quote tick updates.
- `_subscribe_trade_ticks`: Subscribes to trade tick updates.
- `_unsubscribe_trade_ticks`: Unsubscribes from trade tick updates.
- `_subscribe_mark_prices`: Subscribes to mark price updates.
- `_unsubscribe_mark_prices`: Unsubscribes from mark price updates.
- `_subscribe_index_prices`: Subscribes to index price updates.
- `_unsubscribe_index_prices`: Unsubscribes from index price updates.
- `_subscribe_funding_rates`: Subscribes to funding rate updates.
- `_unsubscribe_funding_rates`: Unsubscribes from funding rate updates.
- `_subscribe_bars`: Subscribes to bar/candlestick updates.
- `_unsubscribe_bars`: Unsubscribes from bar updates.
- `_subscribe_instrument_status`: Subscribes to instrument status updates.
- `_unsubscribe_instrument_status`: Unsubscribes from instrument status updates.
- `_subscribe_instrument_close`: Subscribes to instrument close price updates.
- `_unsubscribe_instrument_close`: Unsubscribes from instrument close price updates.
- `_request_instrument`: Requests historical data for a single instrument.
- `_request_instruments`: Requests historical data for multiple instruments.
- `_request_quote_ticks`: Requests historical quote tick data.
- `_request_trade_ticks`: Requests historical trade tick data.
- `_request_bars`: Requests historical bar data.
- `_request_order_book_snapshot`: Requests an order book snapshot.

### ExecutionClient

The `ExecutionClient` is responsible for order management, including submission, modification, and
cancellation of orders. It is a crucial component of the adapter that interacts with the venue
trading system to manage and execute trades.

```python
from nautilus_trader.execution.messages import BatchCancelOrders
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient


class TemplateLiveExecutionClient(LiveExecutionClient):
    """Example `LiveExecutionClient` outlining the required overrides."""

    async def _connect(self) -> None:
        raise NotImplementedError("implement `_connect` in your adapter subclass")

    async def _disconnect(self) -> None:
        raise NotImplementedError("implement `_disconnect` in your adapter subclass")

    async def _submit_order(self, command: SubmitOrder) -> None:
        raise NotImplementedError("implement `_submit_order` in your adapter subclass")

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        raise NotImplementedError("implement `_submit_order_list` in your adapter subclass")

    async def _modify_order(self, command: ModifyOrder) -> None:
        raise NotImplementedError("implement `_modify_order` in your adapter subclass")

    async def _cancel_order(self, command: CancelOrder) -> None:
        raise NotImplementedError("implement `_cancel_order` in your adapter subclass")

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        raise NotImplementedError("implement `_cancel_all_orders` in your adapter subclass")

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        raise NotImplementedError("implement `_batch_cancel_orders` in your adapter subclass")

    async def generate_order_status_report(
        self,
        command: GenerateOrderStatusReport,
    ) -> OrderStatusReport | None:
        raise NotImplementedError("method `generate_order_status_report` must be implemented in the subclass")

    async def generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        raise NotImplementedError("method `generate_order_status_reports` must be implemented in the subclass")

    async def generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        raise NotImplementedError("method `generate_fill_reports` must be implemented in the subclass")

    async def generate_position_status_reports(
        self,
        command: GeneratePositionStatusReports,
    ) -> list[PositionStatusReport]:
        raise NotImplementedError("method `generate_position_status_reports` must be implemented in the subclass")
```

**Key methods**:

- `_connect`: Establishes a connection to the venue APIs.
- `_disconnect`: Closes the connection to the venue APIs.
- `_submit_order`: Submits a new order to the venue.
- `_submit_order_list`: Submits a list of orders to the venue.
- `_modify_order`: Modifies an existing order on the venue.
- `_cancel_order`: Cancels a specific order on the venue.
- `_cancel_all_orders`: Cancels all orders for an instrument on the venue.
- `_batch_cancel_orders`: Cancels a batch of orders for an instrument on the venue.
- `generate_order_status_report`: Generates a report for a specific order on the venue.
- `generate_order_status_reports`: Generates reports for all orders on the venue.
- `generate_fill_reports`: Generates reports for filled orders on the venue.
- `generate_position_status_reports`: Generates reports for position status on the venue.

### Configuration

The configuration file defines settings specific to the adapter, such as API keys and connection
details. These settings are essential for initializing and managing the adapter’s connection to the
data provider.

```python
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig


class TemplateDataClientConfig(LiveDataClientConfig):
    """Configuration for `TemplateDataClient` instances."""

    api_key: str
    api_secret: str
    base_url: str


class TemplateExecClientConfig(LiveExecClientConfig):
    """Configuration for `TemplateExecClient` instances."""

    api_key: str
    api_secret: str
    base_url: str
```

**Key attributes**:

- `api_key`: The API key for authenticating with the data provider.
- `api_secret`: The API secret for authenticating with the data provider.
- `base_url`: The base URL for connecting to the data provider's API.

## Common test scenarios

Exercise adapters across every venue behaviour they claim to support. Incorporate these scenarios into the Rust and Python suites.

### Product coverage

Ensure each supported product family is tested.

- Spot instruments
- Derivatives (perpetuals, futures, swaps)
- Options and structured products

### Order flow

- Cover each supported order type (limit, market, stop, conditional, etc.) under every venue time-in-force option, expiries, and rejection handling.
- Submit buy and sell market orders and assert balance, position, and average-price updates align with venue responses.
- Submit representative buy and sell limit orders, verifying acknowledgements, execution reports, full and partial fills, and cancel flows.

### State management

- Start sessions with existing open orders to ensure the adapter reconciles state on connect before issuing new commands.
- Seed preloaded positions and confirm position snapshots, valuation, and PnL agree with the venue prior to trading.
