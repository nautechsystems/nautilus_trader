# Adapters

## Introduction

This developer guide provides instructions on how to develop an integration adapter for the NautilusTrader platform.
Adapters provide connectivity to trading venues and data providers—translating raw venue APIs into Nautilus’s unified interface and normalized domain model.

## Structure of an adapter

An adapter typically consists of several components:

1. **Instrument Provider**: Supplies instrument definitions
2. **Data Client**: Handles live market data feeds and historical data requests
3. **Execution Client**: Handles order execution and management
4. **Configuration**: Configures the client settings

## Adapter implementation steps

1. Create a new Python subpackage for your adapter
2. Implement the Instrument Provider by inheriting from `InstrumentProvider` and implementing the necessary methods to load instruments
3. Implement the Data Client by inheriting from either the `LiveDataClient` and `LiveMarketDataClient` class as applicable, providing implementations for the required methods
4. Implement the Execution Client by inheriting from `LiveExecutionClient` and providing implementations for the required methods
5. Create configuration classes to hold your adapter’s settings
6. Test your adapter thoroughly to ensure all methods are correctly implemented and the adapter works as expected (see the [Testing Guide](testing.md)).

## Template for building an adapter

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

**Key Methods**:

- `load_all_async`: Loads all instruments asynchronously, optionally applying filters
- `load_ids_async`: Loads specific instruments by their IDs
- `load_async`: Loads a single instrument by its ID

### DataClient

The `LiveDataClient` handles the subscription and management of data feeds that are not specifically
related to market data. This might include news feeds, custom data streams, or other data sources
that enhance trading strategies but do not directly represent market activity.

```python
from nautilus_trader.live.data_client import LiveDataClient
from nautilus_trader.model import DataType
from nautilus_trader.core import UUID4

class TemplateLiveDataClient(LiveDataClient):
    """
    An example of a ``LiveDataClient`` highlighting the overridable abstract methods.
    """

    async def _connect(self) -> None:
        raise NotImplementedError("implement `_connect` in your adapter subclass")

    async def _disconnect(self) -> None:
        raise NotImplementedError("implement `_disconnect` in your adapter subclass")

    def reset(self) -> None:
        raise NotImplementedError("implement `reset` in your adapter subclass")

    def dispose(self) -> None:
        raise NotImplementedError("implement `dispose` in your adapter subclass")

    async def _subscribe(self, data_type: DataType) -> None:
        raise NotImplementedError("implement `_subscribe` in your adapter subclass")

    async def _unsubscribe(self, data_type: DataType) -> None:
        raise NotImplementedError("implement `_unsubscribe` in your adapter subclass")

    async def _request(self, data_type: DataType, correlation_id: UUID4) -> None:
        raise NotImplementedError("implement `_request` in your adapter subclass")
```

**Key Methods**:

- `_connect`: Establishes a connection to the data provider
- `_disconnect`: Closes the connection to the data provider
- `reset`: Resets the state of the client
- `dispose`: Disposes of any resources held by the client
- `_subscribe`: Subscribes to a specific data type
- `_unsubscribe`: Unsubscribes from a specific data type
- `_request`: Requests data from the provider

### MarketDataClient

The `MarketDataClient` handles market-specific data such as order books, top-of-book quotes and trades,
and instrument status updates. It focuses on providing historical and real-time market data that is essential for
trading operations.

```python
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model import BarType, DataType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model.enums import BookType

class TemplateLiveMarketDataClient(LiveMarketDataClient):
    """
    An example of a ``LiveMarketDataClient`` highlighting the overridable abstract methods.
    """

    async def _connect(self) -> None:
        raise NotImplementedError("implement `_connect` in your adapter subclass")

    async def _disconnect(self) -> None:
        raise NotImplementedError("implement `_disconnect` in your adapter subclass")

    def reset(self) -> None:
        raise NotImplementedError("implement `reset` in your adapter subclass")

    def dispose(self) -> None:
        raise NotImplementedError("implement `dispose` in your adapter subclass")

    async def _subscribe_instruments(self) -> None:
        raise NotImplementedError("implement `_subscribe_instruments` in your adapter subclass")

    async def _unsubscribe_instruments(self) -> None:
        raise NotImplementedError("implement `_unsubscribe_instruments` in your adapter subclass")

    async def _subscribe_order_book_deltas(self, instrument_id: InstrumentId, book_type: BookType, depth: int | None = None, kwargs: dict | None = None) -> None:
        raise NotImplementedError("implement `_subscribe_order_book_deltas` in your adapter subclass")

    async def _unsubscribe_order_book_deltas(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError("implement `_unsubscribe_order_book_deltas` in your adapter subclass")
```

**Key Methods**:

- `_connect`: Establishes a connection to the venues APIs
- `_disconnect`: Closes the connection to the venues APIs
- `reset`: Resets the state of the client
- `dispose`: Disposes of any resources held by the client
- `_subscribe_instruments`: Subscribes to market data for multiple instruments
- `_unsubscribe_instruments`: Unsubscribes from market data for multiple instruments
- `_subscribe_order_book_deltas`: Subscribes to order book delta updates
- `_unsubscribe_order_book_deltas`: Unsubscribes from order book delta updates

---

## REST‐API field-mapping guideline

When translating a venue’s REST payload into our domain model **avoid renaming** the upstream
fields unless there is a compelling reason (e.g. a clash with reserved keywords). The only
transformation we apply by default is **camelCase → snake_case**.

Keeping the external names intact makes it trivial to debug payloads, compare captures against the
Rust structs, and speeds up onboarding for new contributors who have the venue’s API reference
open side-by-side.

### ExecutionClient

The `ExecutionClient` is responsible for order management, including submission, modification, and
cancellation of orders. It is a crucial component of the adapter that interacts with the venues
trading system to manage and execute trades.

```python
from nautilus_trader.execution.messages import BatchCancelOrders, CancelAllOrders, CancelOrder, ModifyOrder, SubmitOrder
from nautilus_trader.execution.reports import FillReport, OrderStatusReport, PositionStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model import ClientOrderId, InstrumentId, VenueOrderId

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

    async def _modify_order(self, command: ModifyOrder) -> None:
        raise NotImplementedError("implement `_modify_order` in your adapter subclass")

    async def _cancel_order(self, command: CancelOrder) -> None:
        raise NotImplementedError("implement `_cancel_order` in your adapter subclass")

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        raise NotImplementedError("implement `_cancel_all_orders` in your adapter subclass")

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        raise NotImplementedError("implement `_batch_cancel_orders` in your adapter subclass")

    async def generate_order_status_report(
        self, instrument_id: InstrumentId, client_order_id: ClientOrderId | None = None, venue_order_id: VenueOrderId | None = None
    ) -> OrderStatusReport | None:
        raise NotImplementedError("method `generate_order_status_report` must be implemented in the subclass")

    async def generate_order_status_reports(
        self, instrument_id: InstrumentId | None = None, start: pd.Timestamp | None = None, end: pd.Timestamp | None = None, open_only: bool = False
    ) -> list[OrderStatusReport]:
        raise NotImplementedError("method `generate_order_status_reports` must be implemented in the subclass")

    async def generate_fill_reports(
        self, instrument_id: InstrumentId | None = None, venue_order_id: VenueOrderId | None = None, start: pd.Timestamp | None = None, end: pd.Timestamp | None = None
    ) -> list[FillReport]:
        raise NotImplementedError("method `generate_fill_reports` must be implemented in the subclass")

    async def generate_position_status_reports(
        self, instrument_id: InstrumentId | None = None, start: pd.Timestamp | None = None, end: pd.Timestamp | None = None
    ) -> list[PositionStatusReport]:
        raise NotImplementedError("method `generate_position_status_reports` must be implemented in the subclass")
```

**Key Methods**:

- `_connect`: Establishes a connection to the venues APIs
- `_disconnect`: Closes the connection to the venues APIs
- `_submit_order`: Submits a new order to the venue
- `_modify_order`: Modifies an existing order on the venue
- `_cancel_order`: Cancels a specific order on the venue
- `_cancel_all_orders`: Cancels all orders for an instrument on the venue
- `_batch_cancel_orders`: Cancels a batch of orders for an instrument on the venue
- `generate_order_status_report`: Generates a report for a specific order on the venue
- `generate_order_status_reports`: Generates reports for all orders on the venue
- `generate_fill_reports`: Generates reports for filled orders on the venue
- `generate_position_status_reports`: Generates reports for position status on the venue

### Configuration

The configuration file defines settings specific to the adapter, such as API keys and connection
details. These settings are essential for initializing and managing the adapter’s connection to the
data provider.

```python
from nautilus_trader.config import LiveDataClientConfig, LiveExecClientConfig

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

- `api_key`: The API key for authenticating with the data provider
- `api_secret`: The API secret for authenticating with the data provider
- `base_url`: The base URL for connecting to the data provider’s API
