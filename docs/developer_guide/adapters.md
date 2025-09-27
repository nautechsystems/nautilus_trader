# Adapters

## Introduction

This developer guide provides instructions on how to develop an integration adapter for the NautilusTrader platform.
Adapters provide connectivity to trading venues and data providers—translating raw venue APIs into Nautilus’s unified interface and normalized domain model.

## Structure of an adapter

NautilusTrader adapters follow a layered architecture pattern with a **Rust core** for performance-critical operations
and a **Python layer** for the integration interface. This pattern is consistently used across adapters like Databento, Hyperliquid, OKX, BitMEX, and others.

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
├── config.py             # Configuration classes
├── constants.py          # Adapter constants
├── data.py               # LiveDataClient/LiveMarketDataClient
├── execution.py          # LiveExecutionClient
├── factories.py          # Instrument factories
├── providers.py          # InstrumentProvider
└── __init__.py           # Package initialization
```

## Adapter implementation steps

1. Create a new Python subpackage for your adapter.
2. Implement the Instrument Provider by inheriting from `InstrumentProvider` and implementing the necessary methods to load instruments.
3. Implement the Data Client by inheriting from either the `LiveDataClient` and `LiveMarketDataClient` class as applicable, providing implementations for the required methods.
4. Implement the Execution Client by inheriting from `LiveExecutionClient` and providing implementations for the required methods.
5. Create configuration classes to hold your adapter’s settings.
6. Test your adapter thoroughly to ensure all methods are correctly implemented and the adapter works as expected (see the [Testing Guide](testing.md)).

---

## Rust adapter patterns

- **Common code (`common/`)**: Group venue constants, credential helpers, enums, and reusable parsers under `src/common`. Adapters such as OKX keep submodules like `consts`, `credential`, `enums`, and `urls` alongside a `testing` module for fixtures, providing a single place for cross-cutting pieces.
- **Configurations (`config.rs`)**: Expose typed config structs in `src/config.rs` so Python callers toggle venue-specific behaviour (see how OKX wires demo URLs, retries, and channel flags). Keep defaults minimal and delegate URL selection to helpers in `common::urls`.
- **Error taxonomy (`error.rs`)**: Centralise HTTP/WebSocket failure handling in an adapter-specific error enum. BitMEX, for example, separates retryable, non-retryable, and fatal variants while embedding the original transport error—follow that shape so operational tooling can react consistently.
- **Python exports (`python/mod.rs`)**: Mirror the Rust surface area through PyO3 modules by re-exporting clients, enums, and helper functions. When new functionality lands in Rust, add it to `python/mod.rs` so the Python layer stays in sync (the OKX adapter is a good reference).
- **String interning**: Use `ustr::Ustr` for any non-unique strings the platform stores repeatedly (venues, symbols, instrument IDs) to minimise allocations and comparisons.
- **Testing helpers (`common/testing.rs`)**: Store shared fixtures and payload loaders in `src/common/testing.rs` for use across HTTP and WebSocket unit tests. This keeps `#[cfg(test)]` helpers out of production modules and encourages reuse.

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
```

- Place unit tests next to the module they exercise (`#[cfg(test)]` blocks). Use `src/common/testing.rs` (or an equivalent helper module) for shared fixtures so production files stay tidy.
- Keep Axum-based integration suites under `crates/adapters/<adapter>/tests/`, mirroring the public APIs (HTTP client, WebSocket client, caches, reconnection flows).
- Store upstream payload samples (snapshots, REST replies) under `test_data/` and reference them from both unit and integration tests.

#### Unit tests

- Focus on pure logic: parsers, signing helpers, canonicalisers, and any business rules that do not require a live transport.
- Avoid duplicating coverage that the integration tests already provide.

#### Integration tests

- Drive full request/response cycles through mock servers: authentication, subscription replay, rate-limiting edges, and error branches.
- Prefer event-driven assertions with shared state (for example, collect `subscription_events`, track pending/confirmed topics, wait for `connection_count` transitions) instead of arbitrary `sleep` calls.
- Use `wait_until_async` or adapter-specific helpers to gate on explicit signals such as "auth confirmed" or "reconnection finished" so suites remain deterministic under load.

### Python testing

- Exercise the adapter’s Python surface (instrument providers, data/execution clients, factories) inside `tests/integration_tests/adapters/<adapter>/`.
- Mock the PyO3 boundary (`nautilus_pyo3` shims, stubbed Rust clients) so tests stay fast while verifying that configuration, factory wiring, and error handling match the exported Rust API.
- Align scenarios with the Rust integration coverage: when the Rust suite adds a new behaviour (e.g., subscription replay), mirror the high-level expectations in Python to ensure the full stack stays coherent.

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
    """
    An example template of an ``InstrumentProvider`` showing the minimal methods which must be implemented for an integration to be complete.
    """

    async def load_all_async(self, filters: dict | None = None) -> None:
        raise NotImplementedError("implement `load_all_async` in your adapter subclass")

    async def load_ids_async(self, instrument_ids: list[InstrumentId], filters: dict | None = None) -> None:
        raise NotImplementedError("implement `load_ids_async` in your adapter subclass")

    async def load_async(self, instrument_id: InstrumentId, filters: dict | None = None) -> None:
        raise NotImplementedError("implement `load_async` in your adapter subclass")
```

**Key methods**:

- `load_all_async`: Loads all instruments asynchronously, optionally applying filters
- `load_ids_async`: Loads specific instruments by their IDs
- `load_async`: Loads a single instrument by its ID

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
    """
    An example of a ``LiveDataClient`` highlighting the overridable abstract methods.
    """

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
    """
    An example of a ``LiveMarketDataClient`` highlighting the overridable abstract methods.
    """

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

- `_connect`: Establishes a connection to the venues APIs.
- `_disconnect`: Closes the connection to the venues APIs.
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
cancellation of orders. It is a crucial component of the adapter that interacts with the venues
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
    """
    An example of a ``LiveExecutionClient`` highlighting the method requirements.
    """

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

- `_connect`: Establishes a connection to the venues APIs.
- `_disconnect`: Closes the connection to the venues APIs.
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
    """
    Configuration for ``TemplateDataClient`` instances.
    """

    api_key: str
    api_secret: str
    base_url: str


class TemplateExecClientConfig(LiveExecClientConfig):
    """
    Configuration for ``TemplateExecClient`` instances.
    """

    api_key: str
    api_secret: str
    base_url: str
```

**Key Attributes**:

- `api_key`: The API key for authenticating with the data provider.
- `api_secret`: The API secret for authenticating with the data provider.
- `base_url`: The base URL for connecting to the data provider's API.
