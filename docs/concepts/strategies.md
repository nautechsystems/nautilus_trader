# Strategies

The heart of the NautilusTrader user experience is in writing and working with
trading strategies. Defining a trading strategy is achieved by inheriting the `Strategy` class, 
and implementing the methods required by the strategy.

Using the basic building blocks of data ingest, event handling, and order management (which we will discuss
below), it's possible to implement any type of trading strategy including directional, momentum, re-balancing,
pairs, market making etc.

Refer to the `Strategy` in the [API Reference](../api_reference/trading.md) for a complete description
of all the possible functionality.

There are two main parts of a Nautilus trading strategy:
- The strategy implementation itself, defined by inheriting the `Strategy` class
- The _optional_ strategy configuration, defined by inheriting the `StrategyConfig` class

```{note}
Once a strategy is defined, the same source can be used for backtesting and live trading.
```

The main capabilities of a strategy include:
- Historical data requests
- Live data feed subscriptions
- Setting time alerts or timers
- Accessing the cache
- Accessing the portfolio
- Creating and managing orders

## Implementation
Since a trading strategy is a class which inherits from `Strategy`, you must define
a constructor where you can handle initialization. Minimally the base/super class needs to be initialized:

```python
class MyStrategy(Strategy):
    def __init__(self) -> None:
        super().__init__()  # <-- the super class must be called to initialize the strategy
```

### Handlers

Handlers are methods within the `Strategy` class which may perform actions based on different types of events or state changes.
These methods are named with the prefix `on_*`. You can choose to implement any or all of these handler 
methods depending on the specific needs of your strategy.

The purpose of having multiple handlers for similar types of events is to provide flexibility in handling granularity. 
This means that you can choose to respond to specific events with a dedicated handler, or use a more generic
handler to react to a range of related events (using switch type logic). The call sequence is generally most specific to most general.

#### Stateful actions

These handlers are triggered by lifecycle state changes of the `Strategy`. It's recommended to:

- Use the `on_start` method to initialize your strategy (e.g., fetch instruments, subscribe to data)
- Use the `on_stop` method for cleanup tasks (e.g., unsubscribe from data)

```python
def on_start(self) -> None:
def on_stop(self) -> None:
def on_resume(self) -> None:
def on_reset(self) -> None:
def on_dispose(self) -> None:
def on_degrade(self) -> None:
def on_fault(self) -> None:
def on_save(self) -> dict[str, bytes]:  # Returns user defined dictionary of state to be saved
def on_load(self, state: dict[str, bytes]) -> None:
```

#### Data handling

These handlers deal with market data updates.
You can use these handlers to define actions upon receiving new market data.

```python
def on_order_book_deltas(self, deltas: OrderBookDeltas) -> None:
def on_order_book(self, order_book: OrderBook) -> None:
def on_ticker(self, ticker: Ticker) -> None:
def on_quote_tick(self, tick: QuoteTick) -> None:
def on_trade_tick(self, tick: TradeTick) -> None:
def on_bar(self, bar: Bar) -> None:
def on_venue_status(self, data: VenueStatus) -> None:
def on_instrument(self, instrument: Instrument) -> None:
def on_instrument_status(self, data: InstrumentStatus) -> None:
def on_instrument_close(self, data: InstrumentClose) -> None:
def on_historical_data(self, data: Data) -> None:
def on_data(self, data: Data) -> None:  # Generic data passed to this handler
```

#### Order management

Handlers in this category are triggered by events related to orders.
`OrderEvent` type messages are passed to handlers in this sequence:

1. Specific handler (e.g., on_order_accepted, on_order_rejected, etc.)
2. `on_order_event(...)`
3. `on_event(...)`

```{python}
def on_order_initialized(self, event: OrderInitialized) -> None:
def on_order_denied(self, event: OrderDenied) -> None:
def on_order_emulated(self, event: OrderEmulated) -> None:
def on_order_released(self, event: OrderReleased) -> None:
def on_order_submitted(self, event: OrderSubmitted) -> None:
def on_order_rejected(self, event: OrderRejected) -> None:
def on_order_accepted(self, event: OrderAccepted) -> None:
def on_order_canceled(self, event: OrderCanceled) -> None:
def on_order_expired(self, event: OrderExpired) -> None:
def on_order_triggered(self, event: OrderTriggered) -> None:
def on_order_pending_update(self, event: OrderPendingUpdate) -> None:
def on_order_pending_cancel(self, event: OrderPendingCancel) -> None:
def on_order_modify_rejected(self, event: OrderModifyRejected) -> None:
def on_order_cancel_rejected(self, event: OrderCancelRejected) -> None:
def on_order_updated(self, event: OrderUpdated) -> None:
def on_order_filled(self, event: OrderFilled) -> None:
def on_order_event(self, event: OrderEvent) -> None:  # All order event messages are eventually passed to this handler
```

#### Position management

Handlers in this category are triggered by events related to positions.
`PositionEvent` type messages are passed to handlers in this sequence:

1. Specific handler (e.g., on_position_opened, on_position_changed, etc.)
2. `on_position_event(...)`
3. `on_event(...)`

```python
on_position_opened(self, event: PositionOpened)
on_position_changed(self, event: PositionChanged)
on_position_closed(self, event: PositionClosed)
on_position_event(self, event: PositionEvent)  # All position event messages are eventually passed to this handler
```

#### Generic event handling

This handler will eventually receive all event messages which arrive at the strategy, including those for
which no other specific handler exists.

```python
def on_event(self, event: Event) -> None:
```

## Configuration

The main purpose of a separate configuration class is to provide total flexibility
over where and how a trading strategy can be instantiated. This includes being able
to serialize strategies and their configurations over the wire, making distributed backtesting
and firing up remote live trading possible.

This configuration flexibility is actually opt-in, in that you can actually choose not to have
any strategy configuration beyond the parameters you choose to pass into your
strategies' constructor. However, if you would like to run distributed backtests or launch
live trading servers remotely, then you will need to define a configuration.

Here is an example configuration:

```python
from decimal import Decimal
from nautilus_trader.config import StrategyConfig
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.trading.strategy import Strategy


class MyStrategyConfig(StrategyConfig):
    instrument_id: str
    bar_type: str
    fast_ema_period: int = 10
    slow_ema_period: int = 20
    trade_size: Decimal
    order_id_tag: str

# Here we simply add an instrument ID as a string, to 
# parameterize the instrument the strategy will trade.

class MyStrategy(Strategy):
    def __init__(self, config: MyStrategyConfig) -> None:
        super().__init__(config)

        # Configuration
        self.instrument_id = InstrumentId.from_str(config.instrument_id)


# Once a configuration is defined and instantiated, we can pass this to our 
# trading strategy to initialize.

config = MyStrategyConfig(
    instrument_id="ETHUSDT-PERP.BINANCE",
    bar_type="ETHUSDT-PERP.BINANCE-1000-TICK[LAST]-INTERNAL",
    trade_size=Decimal(1),
    order_id_tag="001",
)

strategy = MyStrategy(config=config)

```

```{note}
Even though it often makes sense to define a strategy which will trade a single
instrument. The number of instruments a single strategy can work with is only limited by machine resources.
```

### Multiple strategies

If you intend running multiple instances of the same strategy, with different
configurations (such as trading different instruments), then you will need to define
a unique `order_id_tag` for each of these strategies (as shown above).

```{note}
The platform has built-in safety measures in the event that two strategies share a
duplicated strategy ID, then an exception will be raised that the strategy ID has already been registered.
```

The reason for this is that the system must be able to identify which strategy
various commands and events belong to. A strategy ID is made up of the
strategy class name, and the strategies `order_id_tag` separated by a hyphen. For
example the above config would result in a strategy ID of `MyStrategy-001`.

```{tip}
See the `StrategyId` [documentation](../api_reference/model/identifiers.md) for further details.
```

### Managed GTD expiry

It's possible for the strategy to manage expiry for orders with a time in force of GTD (_Good 'till Date_).
This may be desirable if the exchange/broker does not support this time in force option, or for any
reason you prefer the strategy to manage this.

To use this option, pass `manage_gtd_expiry=True` to your `StrategyConfig`. When an order is submitted with
a time in force of GTD, the strategy will automatically start an internal time alert.
Once the internal GTD time alert is reached, the order will be canceled (if not already closed).

Some venues (such as Binance Futures) support the GTD time in force, so to avoid conflicts when using
`managed_gtd_expiry` you should set `use_gtd=False` for your execution client config.
