# Strategies

A strategy inherits the `Strategy` class and implements the methods its logic requires.
`Strategy` builds on `DataActor` and adds order management.

**Capabilities**:

- Historical data requests.
- Live data feed subscriptions.
- Setting time alerts or timers.
- Cache access.
- Portfolio access.
- Creating and managing orders and positions.

:::tip
Review the [Actors](actors.md) guide before developing a strategy. It covers the subscription,
request, and callback behavior a strategy inherits.
:::

Add strategies to a Nautilus system in any
[environment context](architecture.md#environment-contexts). They start sending commands and
receiving events based on their logic as soon as the system starts. These building blocks of data
ingest, event handling, and order management (discussed below) support any strategy type, including
directional, momentum, re-balancing, pairs, and market making.

There are two main parts of a Nautilus trading strategy:

- The strategy implementation itself, defined by inheriting the `Strategy` class.
- The *optional* strategy configuration, defined by inheriting the `StrategyConfig` class.

:::tip
Once a strategy is defined, the same source code can be used for backtesting and live trading.
:::

See the [`Strategy` API Reference](/docs/python-api-latest/trading.html) for all available methods.

:::info Rust implementation
Rust strategy authors implement the `DataActor` callbacks they need and use
`nautilus_strategy!` to generate the `Strategy` implementation, then call facade
methods such as `clock()`, `cache()`, `order()`, and `portfolio()` on `self`.
`DataActorNative` is native-only access to runtime wiring and actor-core state;
`StrategyNative` exposes borrowed strategy state such as order factory, order
manager, and portfolio access. Import them only for same-binary performance
paths or internal runtime wiring.
:::

## Strategy implementation

A trading strategy inherits from `Strategy`, so you must define a constructor.
At minimum, initialize the base class:

```python
from nautilus_trader.trading import Strategy


class MyStrategy(Strategy):
    def __init__(self) -> None:
        super().__init__()  # <-- the superclass must be called to initialize the strategy
```

From here, you can implement handlers as necessary to perform actions based on state transitions
and events.

:::warning
`clock`, `cache`, `portfolio`, and `order_factory` raise a `RuntimeError` until the strategy is
registered with a trader, which happens after `__init__` returns. Initialize plain state in the
constructor and do system work in `on_start()`.
:::

### Handlers

Handlers are methods on the `Strategy` class that perform actions based on events or state changes.
These methods use the `on_*` prefix. Implement any or all of them as your strategy requires.

Multiple handlers exist for similar event types to give you control over granularity.
Respond to a specific event with a dedicated handler, or use a generic handler for a range
of related events (using typical switch statement logic).
The system calls handlers in sequence from most specific to most general.

Subscribed data, order, and position handlers dispatch only while the strategy is `RUNNING`.
Messages that arrive in any other state are logged but not passed to your handlers. Request
responses are not gated this way: an `on_historical_*` handler still runs if its response lands
after the strategy stops.

#### Stateful actions

Lifecycle state changes trigger these handlers. Recommendations:

- Use the `on_start` method to initialize your strategy (e.g., fetch instruments, subscribe to data).
- Use the `on_stop` method for cleanup tasks (e.g., cancel open orders, close open positions, unsubscribe from data).

```python
def on_start(self) -> None:
def on_stop(self) -> None:
def on_resume(self) -> None:
def on_reset(self) -> None:
def on_dispose(self) -> None:
def on_degrade(self) -> None:
def on_fault(self) -> None:
def on_save(self) -> dict[str, bytes]:  # Returns user-defined dictionary of state to be saved
def on_load(self, state: dict[str, bytes]) -> None:
```

#### Data handling

These handlers receive data updates, including built-in market data and custom user-defined data.

```python
from collections.abc import Sequence
from typing import Any

from nautilus_trader.common import Signal
from nautilus_trader.model import Bar
from nautilus_trader.model import CustomData
from nautilus_trader.model import FundingRateUpdate
from nautilus_trader.model import IndexPriceUpdate
from nautilus_trader.model import InstrumentClose
from nautilus_trader.model import InstrumentStatus
from nautilus_trader.model import MarkPriceUpdate
from nautilus_trader.model import OptionChainSlice
from nautilus_trader.model import OptionGreeks
from nautilus_trader.model import OrderBook
from nautilus_trader.model import OrderBookDelta
from nautilus_trader.model import OrderBookDeltas
from nautilus_trader.model import OrderBookDepth10
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import TradeTick

def on_book_deltas(self, deltas: OrderBookDeltas) -> None:
def on_book_depth(self, depth: OrderBookDepth10) -> None:
def on_book(self, order_book: OrderBook) -> None:
def on_quote(self, tick: QuoteTick) -> None:
def on_trade(self, tick: TradeTick) -> None:
def on_bar(self, bar: Bar) -> None:
def on_mark_price(self, mark_price: MarkPriceUpdate) -> None:
def on_index_price(self, index_price: IndexPriceUpdate) -> None:
def on_funding_rate(self, funding_rate: FundingRateUpdate) -> None:
def on_instrument(self, instrument: Any) -> None:
def on_instrument_status(self, data: InstrumentStatus) -> None:
def on_instrument_close(self, data: InstrumentClose) -> None:
def on_option_greeks(self, greeks: OptionGreeks) -> None:
def on_option_chain(self, chain: OptionChainSlice) -> None:
def on_historical_data(self, data: CustomData | Sequence[CustomData]) -> None:
def on_historical_book_deltas(self, deltas: Sequence[OrderBookDelta]) -> None:
def on_historical_book_depth(self, depths: Sequence[OrderBookDepth10]) -> None:
def on_historical_quotes(self, quotes: Sequence[QuoteTick]) -> None:
def on_historical_trades(self, trades: Sequence[TradeTick]) -> None:
def on_historical_bars(self, bars: Sequence[Bar]) -> None:
def on_historical_mark_prices(self, mark_prices: Sequence[MarkPriceUpdate]) -> None:
def on_historical_index_prices(self, index_prices: Sequence[IndexPriceUpdate]) -> None:
def on_historical_funding_rates(self, rates: Sequence[FundingRateUpdate]) -> None:
def on_data(self, data: CustomData) -> None:
def on_signal(self, signal: Signal) -> None:
```

Subscribed updates and request responses reach different handlers. See
[Actors: callback handlers](actors.md#callback-handlers) for the operation-to-handler mapping.

#### Order management

These handlers receive events related to orders.
`OrderEvent` type messages are passed to handlers in the following sequence:

1. Specific handler (e.g., `on_order_accepted`, `on_order_rejected`, etc.)
2. `on_order_event(...)`

```python
from typing import Any

from nautilus_trader.model import OrderAccepted
from nautilus_trader.model import OrderCanceled
from nautilus_trader.model import OrderCancelRejected
from nautilus_trader.model import OrderDenied
from nautilus_trader.model import OrderEmulated
from nautilus_trader.model import OrderExpired
from nautilus_trader.model import OrderFilled
from nautilus_trader.model import OrderFillVoided
from nautilus_trader.model import OrderInitialized
from nautilus_trader.model import OrderModifyRejected
from nautilus_trader.model import OrderPendingCancel
from nautilus_trader.model import OrderPendingUpdate
from nautilus_trader.model import OrderRejected
from nautilus_trader.model import OrderReleased
from nautilus_trader.model import OrderSubmitted
from nautilus_trader.model import OrderTriggered
from nautilus_trader.model import OrderUpdated

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
def on_order_fill_voided(self, event: OrderFillVoided) -> None:
def on_order_event(self, event: Any) -> None:  # All order event messages are eventually passed to this handler
```

:::note
The Python API does not export an `OrderEvent` base type. `on_order_event(...)` receives the same
concrete event object the specific handler received, such as an `OrderAccepted`.
:::

#### Position management

These handlers receive events related to positions.
`PositionEvent` type messages are passed to handlers in the following sequence:

1. Specific handler (e.g., `on_position_opened`, `on_position_changed`, etc.)
2. `on_position_event(...)`

```python
from typing import Any

from nautilus_trader.model import PositionChanged
from nautilus_trader.model import PositionClosed
from nautilus_trader.model import PositionOpened

def on_position_opened(self, event: PositionOpened) -> None:
def on_position_changed(self, event: PositionChanged) -> None:
def on_position_closed(self, event: PositionClosed) -> None:
def on_position_event(self, event: Any) -> None:  # All position event messages are eventually passed to this handler
```

As with order events, the Python API does not export a `PositionEvent` base type, so
`on_position_event(...)` receives the concrete event object.

Use `on_time_event()` for timer events, `on_order_event()` for aggregate order events, and
`on_position_event()` for aggregate position events. The Python API does not expose a generic
`on_event()` hook.

#### Handler example

The following example shows a typical `on_start` handler method implementation (taken from the example EMA cross strategy).
Here we can see the following:

- Indicators being registered to receive bar updates.
- Historical data being requested (to hydrate the indicators).
- Live data being subscribed to.

The cache check matters in live trading. Direct subscriptions assume the instrument was
loaded by the instrument provider config or by an earlier instrument request.

```python
def on_start(self) -> None:
    """
    Actions to be performed on strategy start.
    """
    self.instrument = self.cache.instrument(self.instrument_id)
    if self.instrument is None:
        self.log.error(f"Could not find instrument for {self.instrument_id}")
        self.stop()  # Transitions strategy to STOPPED state
        return

    # Register the indicators for updating
    self.register_indicator_for_bars(self.bar_type, self.fast_ema)
    self.register_indicator_for_bars(self.bar_type, self.slow_ema)

    # Get historical data and subscribe to live data
    self.request_bars(self.bar_type)
    self.subscribe_bars(self.bar_type)
    self.subscribe_quotes(self.instrument_id)
```

Registered indicators receive the bars from a request response before `on_historical_bars()` runs,
so a request hydrates them. Check `indicators_initialized()` before acting on indicator values.

### Clock and timers

Strategies have access to a `Clock` which provides a number of methods for creating
different timestamps, as well as setting time alerts or timers to trigger `TimeEvent`s.

See the [`Clock` API Reference](/docs/python-api-latest/common.html) for all available methods.

#### Current timestamps

While there are multiple ways to obtain current timestamps, here are two commonly used methods as examples:

To get the current UTC timestamp as a tz-aware `datetime`:

```python
from datetime import datetime


now: datetime = self.clock.utc_now()
```

To get the current UTC timestamp as nanoseconds since the UNIX epoch:

```python
unix_nanos: int = self.clock.timestamp_ns()
```

#### Time alerts

Time alerts can be set which will result in a `TimeEvent` being dispatched to the `on_time_event` handler at the
specified alert time. In a live context, this might be slightly delayed by a few microseconds.

This example sets a time alert to trigger one minute from the current time:

```python
from datetime import timedelta

# Fire a TimeEvent one minute from now
self.clock.set_time_alert(
    name="MyTimeAlert1",
    alert_time=self.clock.utc_now() + timedelta(minutes=1),
)
```

#### Timers

Continuous timers can be set up which will generate a `TimeEvent` at regular intervals until the timer expires
or is canceled.

This example sets a timer to fire once per minute. The timer starts immediately and its first event
fires one interval later; pass `fire_immediately=True` to fire at the start time instead:

```python
from datetime import timedelta

# Fire a TimeEvent every minute
self.clock.set_timer(
    name="MyTimer1",
    interval=timedelta(minutes=1),
)
```

Pass a `callback` to route `TimeEvent` objects to your own method rather than `on_time_event`. Timer
names share the clock namespace, so use names unique to the component. See
[Actors: timers and alerts](actors.md#timers-and-alerts).

### Cache access

The trader's central `Cache` stores data and execution objects (orders, positions, etc).
Many methods are available with filtering. Here are some basic use cases.

#### Fetching data

The following example fetches data from the cache (assuming some instrument ID attribute is assigned).
These methods return `None` if the requested data is not available.

```python
last_quote = self.cache.quote(self.instrument_id)
last_trade = self.cache.trade(self.instrument_id)
last_bar = self.cache.bar(bar_type)
```

#### Fetching execution objects

The following example shows how individual order and position objects can be fetched from the cache:

```python
order = self.cache.order(client_order_id)
position = self.cache.position(position_id)
```

See the [`Cache` API Reference](/docs/python-api-latest/cache.html) for all available methods.

### Portfolio access

The trader's central `Portfolio` provides account and positional information.
The following shows a general outline of available methods.

#### Account and positional information

```python
import decimal
import typing

from nautilus_trader.model import AccountId
from nautilus_trader.model import Currency
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Money
from nautilus_trader.model import Price
from nautilus_trader.model import Venue

def account(
    self,
    venue: Venue | None = None,
    account_id: AccountId | None = None,
) -> typing.Any | None

def balances_locked(
    self,
    venue: Venue | None = None,
    account_id: AccountId | None = None,
) -> dict[Currency, Money] | None
def instrument_initial_margins(
    self,
    venue: Venue | None = None,
    account_id: AccountId | None = None,
) -> dict[InstrumentId, Money] | None
def instrument_maintenance_margins(
    self,
    venue: Venue | None = None,
    account_id: AccountId | None = None,
) -> dict[InstrumentId, Money] | None
def unrealized_pnls(
    self,
    venue: Venue | None = None,
    account_id: AccountId | None = None,
    target_currency: Currency | None = None,
) -> dict[Currency, Money]
def realized_pnls(
    self,
    venue: Venue | None = None,
    account_id: AccountId | None = None,
    target_currency: Currency | None = None,
) -> dict[Currency, Money]
def total_pnls(
    self,
    venue: Venue | None = None,
    account_id: AccountId | None = None,
    target_currency: Currency | None = None,
) -> dict[Currency, Money]
def net_exposures(
    self,
    venue: Venue | None = None,
    account_id: AccountId | None = None,
    target_currency: Currency | None = None,
) -> dict[Currency, Money] | None

def unrealized_pnl(
    self,
    instrument_id: InstrumentId,
    price: Price | None = None,
    account_id: AccountId | None = None,
    target_currency: Currency | None = None,
) -> Money | None
def realized_pnl(
    self,
    instrument_id: InstrumentId,
    account_id: AccountId | None = None,
    target_currency: Currency | None = None,
) -> Money | None
def total_pnl(
    self,
    instrument_id: InstrumentId,
    price: Price | None = None,
    account_id: AccountId | None = None,
    target_currency: Currency | None = None,
) -> Money | None
def net_exposure(
    self,
    instrument_id: InstrumentId,
    price: Price | None = None,
    account_id: AccountId | None = None,
    target_currency: Currency | None = None,
) -> Money | None
def net_position(
    self,
    instrument_id: InstrumentId,
    account_id: AccountId | None = None,
) -> decimal.Decimal

def is_net_long(self, instrument_id: InstrumentId, account_id: AccountId | None = None) -> bool
def is_net_short(self, instrument_id: InstrumentId, account_id: AccountId | None = None) -> bool
def is_net_flat(self, instrument_id: InstrumentId, account_id: AccountId | None = None) -> bool
def is_completely_net_flat(self, account_id: AccountId | None = None) -> bool
```

If both `venue` and `account_id` are supplied, they must resolve to the same account, otherwise the
query raises `ValueError`. The Portfolio exposes queries to strategies; engine commands remain
internal to the Rust runtime.

See the [`Portfolio` API Reference](/docs/python-api-latest/portfolio.html) for all available methods.

#### Reports and analysis

Use `Portfolio.statistics()` and `Portfolio.snapshots(account_id)` for performance analysis. See the
[Analysis API Reference](/docs/python-api-latest/analysis.html) and
[Portfolio statistics](portfolio.md#portfolio-statistics) guide. The
[Portfolio](portfolio.md) guide also covers equity, mark-to-market valuation, and multi-account
query scope.

### Trading commands

The following trading commands are available for order management.
See also the [Execution](execution.md) guide for the full flow through the system.

#### Submitting orders

An `OrderFactory` is provided on the base class for every `Strategy` as a convenience, reducing
the amount of boilerplate required to create different `Order` objects (although these objects
can still be initialized directly with the `Order.__init__(...)` constructor if the trader prefers).

The component a `SubmitOrder` or `SubmitOrderList` command will flow to for execution depends on the following:

- If an `emulation_trigger` is specified, the command will *firstly* be sent to the `OrderEmulator`.
- If an `exec_algorithm_id` is specified (with no `emulation_trigger`), the command will *firstly* be sent to the relevant `ExecutionAlgorithm`.
- Otherwise, the command will *firstly* be sent to the `RiskEngine`.

This example submits a `LIMIT` BUY order for emulation (see [Emulated Orders](orders/emulated.md)):

```python
from nautilus_trader.model import LimitOrder
from nautilus_trader.model import OrderSide
from nautilus_trader.model import TriggerType


def buy(self) -> None:
    """
    Users simple buy method (example).
    """
    order: LimitOrder = self.order_factory.limit(
        instrument_id=self.instrument_id,
        order_side=OrderSide.BUY,
        quantity=self.instrument.make_qty(self.trade_size),
        price=self.instrument.make_price(5000.00),
        emulation_trigger=TriggerType.LAST_PRICE,
    )

    self.submit_order(order)
```

:::info
You can specify both order emulation and an execution algorithm. In this case, the order is
first sent to the `OrderEmulator`, and upon release is then routed to the `ExecutionAlgorithm`.
:::

This example submits a `MARKET` BUY order to a TWAP execution algorithm:

```python
from nautilus_trader.model import ExecAlgorithmId
from nautilus_trader.model import MarketOrder
from nautilus_trader.model import OrderSide
from nautilus_trader.model import TimeInForce


def buy(self) -> None:
    """
    Users simple buy method (example).
    """
    order: MarketOrder = self.order_factory.market(
        instrument_id=self.instrument_id,
        order_side=OrderSide.BUY,
        quantity=self.instrument.make_qty(self.trade_size),
        time_in_force=TimeInForce.FOK,
        exec_algorithm_id=ExecAlgorithmId("TWAP"),
        exec_algorithm_params={"horizon_secs": "20", "interval_secs": "2.5"},
    )

    self.submit_order(order)
```

#### Canceling orders

Orders can be canceled individually, as a batch, or all orders for an instrument (with an optional side filter).

If the order is already *closed* or already pending cancel, then a warning will be logged.

If the order is currently *open* then the status will become `PENDING_CANCEL`.

Routing depends on the command and on the state of each order:

- `cancel_order(...)` goes *firstly* to the `OrderEmulator` when the order is emulated, to the
  relevant `ExecutionAlgorithm` when the order has an `exec_algorithm_id` and is still active within
  the local system, and to the `ExecutionEngine` otherwise.
- `cancel_all_orders(...)` fans out: open and in-flight orders go to the `ExecutionEngine`, emulated
  orders go to the `OrderEmulator`, and every execution algorithm order is canceled individually.
- `cancel_orders(...)` always goes to the `ExecutionEngine` as a single `BatchCancelOrders` command.

:::info
Any managed GTD timer will also be canceled after the command has left the strategy.
:::

The following shows how to cancel an individual order:

```python
self.cancel_order(order.client_order_id)
```

The following shows how to cancel a batch of orders. Every order in the batch must be for the same
instrument, and the batch must not include emulated or local orders:

```python
from nautilus_trader.model import ClientOrderId


client_order_ids: list[ClientOrderId] = [
    order1.client_order_id,
    order2.client_order_id,
    order3.client_order_id,
]
self.cancel_orders(client_order_ids)
```

The following shows how to cancel all orders:

```python
self.cancel_all_orders(self.instrument_id)
```

#### Modifying orders

Orders can be modified individually when emulated, or *open* on a venue (if supported).

If the order is already *closed* or already pending cancel, then a warning will be logged.
If the order is currently *open* then the status will become `PENDING_UPDATE`.

:::warning
At least one value must differ from the original order for the command to be valid.
:::

The component a `ModifyOrder` command will flow to for execution depends on the following:

- If the order is currently emulated, the command will *firstly* be sent to the `OrderEmulator`.
- Otherwise, the order will *firstly* be sent to the `RiskEngine`.

:::info
Unlike `CancelOrder`, a `ModifyOrder` command never routes to an execution algorithm.
:::

The following shows how to modify the size of `LIMIT` BUY order currently *open* on a venue:

```python
from nautilus_trader.model import Quantity


new_quantity: Quantity = Quantity.from_int(5)
self.modify_order(order.client_order_id, quantity=new_quantity)
```

:::info
The price and trigger price can also be modified (when emulated or supported by a venue).
:::

Use `modify_orders(...)` to send several modifications as a single `BatchModifyOrders` command to
the `RiskEngine`. As with a batch cancel, every order must be for the same instrument, and the batch
must not include emulated or local orders.

#### Market exit

The `market_exit()` method provides a graceful way to exit all positions and cancel all orders
for a strategy. The strategy remains running after the exit completes, allowing you to re-enter
positions later if desired.

```python
self.market_exit()
```

The call logs a warning and returns without effect if the strategy is not `RUNNING`, or if an exit
is already in progress.

The market exit process:

1. Calls `on_market_exit()`.
2. Cancels all open and in-flight orders for the strategy.
3. Closes all open positions with market orders tagged `MARKET_EXIT`.
4. Periodically checks (at `market_exit_interval_ms`) until all orders resolve and positions close,
   re-submitting a closing order for any position still open once no orders remain working.
5. Calls `post_market_exit()` once flat, or after `market_exit_max_attempts` is reached, logging the
   orders and positions still outstanding.

Two hooks are available for custom logic:

- `on_market_exit()`: called when the exit process begins.
- `post_market_exit()`: called when the exit process completes.

```python
class MyStrategy(Strategy):
    def on_market_exit(self) -> None:
        self.log.info("Beginning market exit...")

    def post_market_exit(self) -> None:
        self.log.info("Market exit complete")
```

During a market exit, non-reduce-only orders are automatically denied with the reason
`MARKET_EXIT_IN_PROGRESS`. The exit's own closing orders pass through because they carry the
`MARKET_EXIT` tag. For order lists, if any order in the list is non-reduce-only, the entire list is
denied to preserve list semantics (e.g., bracket orders with interdependencies).

To check if an exit is in progress (e.g., to skip order submission logic), use `is_exiting()`:

```python
def on_quote(self, tick: QuoteTick) -> None:
    if self.is_exiting():
        return  # Skip order logic during exit
    # ... normal order logic
```

To automatically perform a market exit when the strategy is stopped, set `manage_stop=True`:

```python
config = StrategyConfig(manage_stop=True)
```

With this option, calling `stop()` will first perform a market exit, then stop the strategy
once flat.

Configuration options in `StrategyConfig`:

- `manage_stop` (default: `False`): if `True`, `stop()` performs a market exit before stopping.
- `market_exit_interval_ms` (default: `100`): interval between exit completion checks.
- `market_exit_max_attempts` (default: `100`): maximum checks before completing the exit.
- `market_exit_time_in_force` (default: `GTC`): time in force for closing market orders.
- `market_exit_reduce_only` (default: `True`): if closing market orders should be reduce only.

#### Closing positions

Use `close_position(...)` and `close_all_positions(...)` to flatten without running the full market
exit process. Both submit closing market orders and leave the strategy free to submit new orders.
See the [Execution](execution.md) guide.

## Strategy configuration

A separate configuration class gives full flexibility over where and how a strategy
is instantiated. Configurations serialize over the wire, enabling distributed backtesting
and remote live trading.

This is opt-in. You can skip configuration and pass parameters directly to your
strategy constructor. If you want distributed backtests or remote live trading,
define a configuration.

`StrategyConfig` is implemented in Rust: `__new__` builds and validates the base fields, and
`__init__` does nothing. A subclass therefore assigns its own fields in `__init__` and forwards base
fields such as `strategy_id` and `order_id_tag` through `__new__`. Popping the subclass fields in
`__new__` keeps a custom field from being matched against a base field of the same name.

Here is an example configuration:

```python
from decimal import Decimal

from nautilus_trader.config import StrategyConfig
from nautilus_trader.model import Bar
from nautilus_trader.model import BarType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import StrategyId
from nautilus_trader.trading import Strategy


# Configuration definition
class MyStrategyConfig(StrategyConfig):
    _CUSTOM_FIELDS = (
        "instrument_id",
        "bar_type",
        "fast_ema_period",
        "slow_ema_period",
        "trade_size",
    )

    def __new__(cls, *args, **kwargs):
        for field in cls._CUSTOM_FIELDS:
            kwargs.pop(field, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(
        self,
        instrument_id: InstrumentId,
        bar_type: BarType,
        trade_size: Decimal,
        fast_ema_period: int = 10,
        slow_ema_period: int = 20,
        **_kwargs,
    ) -> None:
        super().__init__()
        self.instrument_id = instrument_id
        self.bar_type = bar_type
        self.trade_size = trade_size
        self.fast_ema_period = fast_ema_period
        self.slow_ema_period = slow_ema_period


# Strategy definition
class MyStrategy(Strategy):
    def __init__(self, config: MyStrategyConfig) -> None:
        # Always initialize the parent Strategy class
        # After this, configuration is stored and available via `self.config`
        super().__init__(config)

        # Custom state variables
        self.time_started = None
        self.count_of_processed_bars: int = 0

    def on_start(self) -> None:
        self.time_started = self.clock.utc_now()  # Remember time, when strategy started
        self.subscribe_bars(
            self.config.bar_type
        )  # See how configuration data are exposed via `self.config`

    def on_bar(self, bar: Bar):
        self.count_of_processed_bars += 1  # Update count of processed bars


# Instantiate configuration with specific values. By setting:
#   - InstrumentId - we parameterize the instrument the strategy will trade.
#   - BarType - we parameterize bar-data, that strategy will trade.
#   - StrategyId - we name this instance, which also fixes its order ID tag.
config = MyStrategyConfig(
    instrument_id=InstrumentId.from_str("ETHUSDT-PERP.BINANCE"),
    bar_type=BarType.from_str("ETHUSDT-PERP.BINANCE-15-MINUTE-LAST-EXTERNAL"),
    trade_size=Decimal("1"),
    strategy_id=StrategyId("MyStrategy-001"),
)

# Pass configuration to our trading strategy.
strategy = MyStrategy(config=config)
```

Access configuration values through `self.config`.
This provides clear separation between:

- Configuration data (accessed via `self.config`):
  - Contains initial settings, that define how the strategy works.
  - Example: `self.config.trade_size`, `self.config.instrument_id`

- Strategy state variables (as direct attributes):
  - Track any custom state of the strategy.
  - Example: `self.time_started`, `self.count_of_processed_bars`

This separation makes code easier to understand and maintain.

:::note
Even though it often makes sense to define a strategy which will trade a single
instrument. The number of instruments a single strategy can work with is only limited by machine resources.
:::

### Managed GTD expiry

It's possible for the strategy to manage expiry for orders with a time in force of GTD (*Good 'till Date*).
This may be desirable if the exchange/broker does not support this time in force option, or for any
reason you prefer the strategy to manage this.

To use this option, pass `manage_gtd_expiry=True` to your `StrategyConfig`. When an order is submitted with
a time in force of GTD, the strategy will automatically start an internal time alert.
Once the internal GTD time alert is reached, the order will be canceled (if not already *closed*).

On start, the strategy also reinstates alerts for its open GTD orders held in the cache, and cancels
any whose expiry has already passed.

Some venues (such as Binance Futures) support the GTD time in force, so to avoid conflicts when using
`manage_gtd_expiry` you should set `use_gtd=False` for your execution client config.

### Multiple strategies

If you intend running multiple instances of the same strategy, with different
configurations (such as trading different instruments), then each instance needs a
unique strategy ID and order ID tag.

The system must be able to identify which strategy various commands and events belong to. The order
ID tag also keeps generated client order IDs unique across strategies for the same trader.

Set `strategy_id` on each config. The runtime takes the order ID tag from the final
hyphen-separated part of the strategy ID, so `MyStrategy-001` and `MyStrategy-002` produce the tags
`001` and `002`.

Supplying `order_id_tag` as well appends the tag to the runtime strategy ID, unless the ID already
ends with that tag. For example, `strategy_id=StrategyId("MyStrategy-PRIMARY")` with
`order_id_tag="ABC"` registers as `MyStrategy-PRIMARY-ABC`.

A strategy registered without `strategy_id` takes its base ID from the strategy type name. An
`order_id_tag` becomes the suffix, so `MyStrategy` with `order_id_tag="ABC"` registers as
`MyStrategy-ABC`; without a tag, registration assigns the next numeric tag, starting with `000`.

:::note
The platform has built-in safety measures. Registering a duplicated strategy ID raises a
`RuntimeError` indicating the strategy ID is already registered, and two different strategy IDs that
share an order ID tag raise a `RuntimeError` reporting the tag conflict.
:::

:::info Rust implementation
Rust treats `StrategyConfig` as immutable construction input. The runtime
`StrategyId` carries the order ID tag, matching the Python behavior. This keeps
actor registration, client order ID generation, order list ID generation, and
position ID generation aligned through `strategy_id.get_tag()`.
:::

See the [`StrategyId` API Reference](/docs/python-api-latest/model/identifiers.html) for further details.

## Related guides

- [Actors](actors.md) - Base class that strategies extend.
- [Events](events/) - Event types and handler dispatch.
- [Orders](orders/) - Order types and management from strategies.
- [Backtesting](backtesting/) - Test strategies with historical data.
