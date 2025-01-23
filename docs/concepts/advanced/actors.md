# Actors

:::info
We are currently working on this guide.
:::

The `Actor` class provides the foundation for components that can interact with the trading system,
including `Strategy` which inherits from it and additionally provides order management
methods on top. This means everything discussed in the [Strategies](../strategies.md) guide
also applies to actors.

## Basic Example

Just like strategies, actors support configuration through very similar pattern.

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

## Data Handling and Callbacks

When working with data in Nautilus, it's important to understand the relationship between data requests/subscriptions and their corresponding callback handlers. The system uses different handlers depending on whether the data is historical or real-time.

### Historical vs Real-time Data

The system distinguishes between two types of data flow:

1. **Historical Data** (from requests):
   - Obtained through methods like `request_bars()`, `request_quote_ticks()`, etc.
   - Processed through the `on_historical_data()` handler.
   - Used for initial data loading and historical analysis.

2. **Real-time Data** (from subscriptions):
   - Obtained through methods like `subscribe_bars()`, `subscribe_quote_ticks()`, etc.
   - Processed through specific handlers like `on_bar()`, `on_quote_tick()`, etc.
   - Used for live data processing.

### Callback Handlers

Here's how different data operations map to their handlers:

| Operation | Method | Handler | Purpose |
|:----------|:---------|:---------|:----------|
| `request_bars()` | Historical request | `on_historical_data()` | Process historical bars |
| `subscribe_bars()` | Real-time subscription | `on_bar()` | Process live bar updates |
| `request_quote_ticks()` | Historical request | `on_historical_data()` | Process historical quotes |
| `subscribe_quote_ticks()` | Real-time subscription | `on_quote_tick()` | Process live quote updates |
| `request_trade_ticks()` | Historical request | `on_historical_data()` | Process historical trades |
| `subscribe_trade_ticks()` | Real-time subscription | `on_trade_tick()` | Process live trade updates |

### Example

Here's an example demonstrating both historical and real-time data handling:

```python
from nautilus_trader.common.actor import Actor
from nautilus_trader.config import ActorConfig
from nautilus_trader.core.data import Data
from nautilus_trader.model.data import Bar, BarType
from nautilus_trader.model.identifiers import ClientId, InstrumentId


class MyActorConfig(ActorConfig):
    instrument_id: InstrumentId   # example value: "AAPL.NASDAQ"
    bar_type: BarType            # example value: "AAPL.NASDAQ-1-MINUTE-LAST-EXTERNAL"


class MyActor(Actor):
    def __init__(self, config: MyActorConfig) -> None:
        super().__init__(config)
        self.bar_type = config.bar_type

    def on_start(self) -> None:
        # Request historical data - will be processed by on_historical_data() handler
        self.request_bars(
            bar_type=self.bar_type,
            # Many optional parameters
            start=None,           # datetime, optional
            end=None,             # datetime, optional
            callback=None,        # custom handler function, optional
            update_catalog=False, # bool, default False
            params=None,          # dict[str, Any], optional
        )

        # Subscribe to real-time data - will be processed by on_bar() handler
        self.subscribe_bars(
            bar_type=self.bar_type,
            # Many optional parameters
            client_id=None,      # ClientId, optional
            await_partial=False, # bool, default False
            params=None,         # dict[str, Any], optional
        )

    def on_historical_data(self, data: Data) -> None:
        # Handle historical data (from requests)
        if isinstance(data, Bar):
            self.log.info(f"Received historical bar: {data}")

    def on_bar(self, bar: Bar) -> None:
        # Handle real-time bar updates (from subscriptions)
        self.log.info(f"Received real-time bar: {bar}")
```

This separation between historical and real-time data handlers allows for different processing logic based on the data
context. For example, you might want to:

- Use historical data to initialize indicators or establish baseline metrics.
- Process real-time data differently for live trading decisions.
- Apply different validation or logging for historical vs real-time data.

:::tip
When debugging data flow issues, check that you're looking at the correct handler for your data source.
If you're not seeing data in `on_bar()` but see log messages about receiving bars, check `on_historical_data()`
as the data might be coming from a request rather than a subscription.
:::
