# Actors

:::info
We are currently working on this concept guide.
:::

The `Actor` serves as the foundational component for interacting with the trading system.
It provides core functionality for receiving market data, handling events, and managing state within
the trading environment. The `Strategy` class inherits from Actor and extends its capabilities with
order management methods.

**Key capabilities**:

- Event subscription and handling.
- Market data reception.
- State management.
- System interaction primitives.

## Basic example

Just like strategies, actors support configuration through a very similar pattern.

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
        # Subscribe to all incoming bars
        self.subscribe_bars(self.config.bar_type)   # You can access configuration directly via `self.config`

    def on_bar(self, bar: Bar):
        self.count_of_processed_bars += 1
```

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
| `subscribe_order_book_deltas()` | Real‑time        | `on_order_book_deltas()` | Live order book updates. |
| `subscribe_quote_ticks()`       | Real‑time        | `on_quote_tick()`        | Live quote updates. |
| `subscribe_trade_ticks()`       | Real‑time        | `on_trade_tick()`        | Live trade updates. |
| `subscribe_mark_prices()`       | Real‑time        | `on_mark_price()`        | Live mark price updates. |
| `subscribe_index_prices()`      | Real‑time        | `on_index_price()`       | Live index price updates. |
| `subscribe_funding_rates()`     | Real‑time        | `on_funding_rate()`      | Live funding rate updates. |
| `subscribe_bars()`              | Real‑time        | `on_bar()`               | Live bar updates. |
| `subscribe_instrument_status()` | Real‑time        | `on_instrument_status()` | Live instrument status updates. |
| `subscribe_instrument_close()`  | Real‑time        | `on_instrument_close()`  | Live instrument close updates. |
| `subscribe_order_fills()`       | Real‑time        | `on_order_filled()`      | Live order fill events for an instrument. |
| `request_data()`                | Historical       | `on_historical_data()`   | Historical data processing. |
| `request_instrument()`          | Historical       | `on_instrument()`        | Instrument definition updates. |
| `request_instruments()`         | Historical       | `on_instrument()`        | Instrument definition updates. |
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
            start=None,                # datetime, optional
            end=None,                  # datetime, optional
            callback=None,             # called with the request ID when completed
            update_catalog_mode=None,  # UpdateCatalogMode | None, default None
            params=None,               # dict[str, Any], optional
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
