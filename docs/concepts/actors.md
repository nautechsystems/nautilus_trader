# Actors

An `Actor` receives data, handles events, and manages state. The `Strategy` class extends Actor
with order management capabilities.

**Key capabilities**:

- Data subscription and requests (market data, custom data).
- Event handling and publishing.
- Timers and alerts.
- Cache and portfolio access.
- Logging.

## Basic example

Actors support configuration through a pattern similar to strategies.

```python
from nautilus_trader.config import ActorConfig
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Bar, BarType
from nautilus_trader.common.actor import Actor


class MyActorConfig(ActorConfig):
    instrument_id: InstrumentId   # example value: "ETHUSDT-PERP.BINANCE"
    bar_type: BarType             # example value: "ETHUSDT-PERP.BINANCE-15-MINUTE[LAST]-INTERNAL"
    lookback_period: int = 10


class MyActor(Actor):
    def __init__(self, config: MyActorConfig) -> None:
        super().__init__(config)

        # Custom state variables
        self.count_of_processed_bars: int = 0

    def on_start(self) -> None:
        # Subscribe to bars matching the configured bar type
        self.subscribe_bars(self.config.bar_type)

    def on_bar(self, bar: Bar) -> None:
        self.count_of_processed_bars += 1
```

## Lifecycle

Actors follow a defined state machine through their lifecycle:

```mermaid
stateDiagram-v2
    [*] --> PRE_INITIALIZED
    PRE_INITIALIZED --> READY : register()
    READY --> STARTING : start()
    STARTING --> RUNNING : on_start()
    RUNNING --> STOPPING : stop()
    STOPPING --> STOPPED : on_stop()
    STOPPED --> RUNNING : resume()
    RUNNING --> DEGRADING : degrade()
    DEGRADING --> DEGRADED : on_degrade()
    DEGRADED --> RUNNING : resume()
    RUNNING --> FAULTING : fault()
    FAULTING --> FAULTED : on_fault()
    RUNNING --> DISPOSED : dispose()
```

Override these methods to hook into lifecycle events:

| Method          | When called                                                         |
|-----------------|---------------------------------------------------------------------|
| `on_start()`    | Actor is starting (subscribe to data here).                         |
| `on_stop()`     | Actor is stopping (cancel timers, cleanup resources).               |
| `on_resume()`   | Actor is resuming from a stopped state.                             |
| `on_reset()`    | Reset indicators and internal state (called between backtest runs). |
| `on_degrade()`  | Actor is entering a degraded state (partial functionality).         |
| `on_fault()`    | Actor has encountered a critical fault.                             |
| `on_dispose()`  | Actor is being disposed (final cleanup).                            |

## Timers and alerts

Actors have access to a clock for scheduling:

```python
def on_start(self) -> None:
    # Set a recurring timer (fires every 5 seconds)
    self.clock.set_timer("my_timer", timedelta(seconds=5))

    # Set a one-time alert
    self.clock.set_alert("my_alert", self.clock.utc_now() + timedelta(minutes=1))

def on_stop(self) -> None:
    # Cancel timers to prevent resource leaks across stop/resume cycles
    self.clock.cancel_timer("my_timer")

def on_timer(self, event: TimeEvent) -> None:
    if event.name == "my_timer":
        self.log.info("Timer fired!")

def on_alert(self, event: TimeEvent) -> None:
    if event.name == "my_alert":
        self.log.info("Alert triggered!")
```

## System access

Actors have access to core system components:

| Property          | Description                                              |
|-------------------|----------------------------------------------------------|
| `self.cache`      | Read-only access to instruments, orders, positions, etc. |
| `self.portfolio`  | Portfolio state and calculations.                        |
| `self.clock`      | Current time and timer/alert scheduling.                 |
| `self.log`        | Structured logging.                                      |
| `self.msgbus`     | Publish/subscribe to custom messages.                    |

For custom messaging between components, see the [Message Bus](message_bus.md) guide.

## Data handling and callbacks

When working with data in Nautilus, it's important to understand the relationship between data
*requests/subscriptions* and their corresponding callback handlers. The system uses different handlers
depending on whether the data is historical or real-time.

### Historical vs real-time data

The system distinguishes between two types of data flow:

1. **Historical data** (from *requests*):
   - Obtained through methods like `request_bars()`, `request_quote_ticks()`, etc.
   - Processed through the `on_historical_data()` handler.
   - Used for initial data loading and historical analysis.

2. **Real-time data** (from *subscriptions*):
   - Obtained through methods like `subscribe_bars()`, `subscribe_quote_ticks()`, etc.
   - Processed through specific handlers like `on_bar()`, `on_quote_tick()`, etc.
   - Used for live data processing.

### Callback handlers

Here's how different data operations map to their handlers:

| Operation                       | Category         | Handler                  | Purpose |
|:--------------------------------|:-----------------|:-------------------------|:--------|
| `subscribe_data()`              | Real‑time        | `on_data()`              | Live data updates. |
| `subscribe_instrument()`        | Real‑time        | `on_instrument()`        | Live instrument definition updates. |
| `subscribe_instruments()`       | Real‑time        | `on_instrument()`        | Live instrument definition updates (for venue). |
| `subscribe_order_book_deltas()` | Real‑time        | `on_order_book_deltas()` | Live order book deltas. |
| `subscribe_order_book_depth()`  | Real‑time        | `on_order_book_depth()`  | Live order book depth snapshots. |
| `subscribe_order_book_at_interval()` | Real‑time   | `on_order_book()`        | Live order book snapshots at intervals. |
| `subscribe_quote_ticks()`       | Real‑time        | `on_quote_tick()`        | Live quote updates. |
| `subscribe_trade_ticks()`       | Real‑time        | `on_trade_tick()`        | Live trade updates. |
| `subscribe_mark_prices()`       | Real‑time        | `on_mark_price()`        | Live mark price updates. |
| `subscribe_index_prices()`      | Real‑time        | `on_index_price()`       | Live index price updates. |
| `subscribe_funding_rates()`     | Real‑time        | `on_funding_rate()`      | Live funding rate updates. |
| `subscribe_bars()`              | Real‑time        | `on_bar()`               | Live bar updates. |
| `subscribe_instrument_status()` | Real‑time        | `on_instrument_status()` | Live instrument status updates. |
| `subscribe_instrument_close()`  | Real‑time        | `on_instrument_close()`  | Live instrument close updates. |
| `subscribe_order_fills()`       | Real‑time        | `on_order_filled()`      | Live order fill events for an instrument. |
| `subscribe_order_cancels()`     | Real‑time        | `on_order_canceled()`    | Live order cancel events for an instrument. |
| `request_data()`                | Historical       | `on_historical_data()`   | Historical data processing. |
| `request_order_book_snapshot()` | Historical       | `on_historical_data()`   | Historical order book snapshot. |
| `request_order_book_depth()`    | Historical       | `on_historical_data()`   | Historical order book depth. |
| `request_instrument()`          | Historical       | `on_instrument()`        | Instrument definition. |
| `request_instruments()`         | Historical       | `on_instrument()`        | Instrument definitions. |
| `request_quote_ticks()`         | Historical       | `on_historical_data()`   | Historical quotes processing. |
| `request_trade_ticks()`         | Historical       | `on_historical_data()`   | Historical trades processing. |
| `request_bars()`                | Historical       | `on_historical_data()`   | Historical bars processing. |
| `request_aggregated_bars()`     | Historical       | `on_historical_data()`   | Historical aggregated bars (on-the-fly). |

### Example

Here's an example demonstrating both historical and real-time data handling:

```python
from nautilus_trader.common.actor import Actor
from nautilus_trader.config import ActorConfig
from nautilus_trader.core.data import Data
from nautilus_trader.model import Bar, BarType
from nautilus_trader.model import ClientId, InstrumentId


class MyActorConfig(ActorConfig):
    instrument_id: InstrumentId  # example value: "AAPL.XNAS"
    bar_type: BarType            # example value: "AAPL.XNAS-1-MINUTE-LAST-EXTERNAL"


class MyActor(Actor):
    def __init__(self, config: MyActorConfig) -> None:
        super().__init__(config)
        self.bar_type = config.bar_type

    def on_start(self) -> None:
        # Request historical data - will be processed by on_historical_data() handler
        self.request_bars(
            bar_type=self.bar_type,
            # Many optional parameters
            start=None,                # pd.Timestamp | None
            end=None,                  # pd.Timestamp | None
            callback=None,             # Callable[[UUID4], None] | None
            update_catalog_mode=None,  # UpdateCatalogMode | None
            params=None,               # dict[str, Any] | None
        )

        # Subscribe to real-time data - will be processed by on_bar() handler
        self.subscribe_bars(
            bar_type=self.bar_type,
            # Many optional parameters
            client_id=None,  # ClientId, optional
            params=None,     # dict[str, Any], optional
        )

    def on_historical_data(self, data: Data) -> None:
        # Handle historical data (from requests)
        if isinstance(data, Bar):
            self.log.info(f"Received historical bar: {data}")

    def on_bar(self, bar: Bar) -> None:
        # Handle real-time bar updates (from subscriptions)
        self.log.info(f"Received real-time bar: {bar}")
```

This separation between historical and real-time data handlers allows for different processing logic
based on the data context. For example, you might want to:

- Use historical data to initialize indicators or establish baseline metrics.
- Process real-time data differently for live trading decisions.
- Apply different validation or logging for historical vs real-time data.

:::tip
When debugging data flow issues, check that you're looking at the correct handler for your data source.
If you're not seeing data in `on_bar()` but see log messages about receiving bars, check `on_historical_data()`
as the data might be coming from a request rather than a subscription.
:::

## Order fill subscriptions

Actors can subscribe to order fill events for specific instruments using `subscribe_order_fills()`. This is useful
for monitoring trading activity, implementing custom fill analysis, or tracking execution quality.

When subscribed, all order fills for the specified instrument are forwarded to the `on_order_filled()` handler,
regardless of which strategy or component generated the original order.

### Example

```python
from nautilus_trader.common.actor import Actor
from nautilus_trader.config import ActorConfig
from nautilus_trader.model import InstrumentId
from nautilus_trader.model.events import OrderFilled


class MyActorConfig(ActorConfig):
    instrument_id: InstrumentId  # example value: "ETHUSDT-PERP.BINANCE"


class FillMonitorActor(Actor):
    def __init__(self, config: MyActorConfig) -> None:
        super().__init__(config)
        self.fill_count = 0
        self.total_volume = 0.0

    def on_start(self) -> None:
        # Subscribe to all fills for the instrument
        self.subscribe_order_fills(self.config.instrument_id)

    def on_order_filled(self, event: OrderFilled) -> None:
        # Handle order fill events
        self.fill_count += 1
        self.total_volume += float(event.last_qty)

        self.log.info(
            f"Fill received: {event.order_side} {event.last_qty} @ {event.last_px}, "
            f"Total fills: {self.fill_count}, Volume: {self.total_volume}"
        )

    def on_stop(self) -> None:
        # Unsubscribe from fills
        self.unsubscribe_order_fills(self.config.instrument_id)
```

:::note
Order fill subscriptions are message bus-only subscriptions and do not involve the data engine.
The `on_order_filled()` handler will only receive events while the actor is in a running state.
:::

## Order cancel subscriptions

Actors can subscribe to order cancel events for specific instruments using `subscribe_order_cancels()`. This is useful
for monitoring order cancellations, implementing custom cancel analysis, or tracking order lifecycle events.

When subscribed, all order cancels for the specified instrument are forwarded to the `on_order_canceled()` handler,
regardless of which strategy or component generated the original order.

### Example

```python
from nautilus_trader.common.actor import Actor
from nautilus_trader.config import ActorConfig
from nautilus_trader.model import InstrumentId
from nautilus_trader.model.events import OrderCanceled


class MyActorConfig(ActorConfig):
    instrument_id: InstrumentId  # example value: "ETHUSDT-PERP.BINANCE"


class CancelMonitorActor(Actor):
    def __init__(self, config: MyActorConfig) -> None:
        super().__init__(config)
        self.cancel_count = 0

    def on_start(self) -> None:
        # Subscribe to all cancels for the instrument
        self.subscribe_order_cancels(self.config.instrument_id)

    def on_order_canceled(self, event: OrderCanceled) -> None:
        # Handle order cancel events
        self.cancel_count += 1

        self.log.info(
            f"Cancel received: {event.client_order_id}, "
            f"Total cancels: {self.cancel_count}"
        )

    def on_stop(self) -> None:
        # Unsubscribe from cancels
        self.unsubscribe_order_cancels(self.config.instrument_id)
```

:::note
Order cancel subscriptions are message bus-only subscriptions and do not involve the data engine.
The `on_order_canceled()` handler will only receive events while the actor is in a running state.
:::
