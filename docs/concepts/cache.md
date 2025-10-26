# Cache

The `Cache` is a central in-memory database that automatically stores and manages all trading-related data.
Think of it as your trading system’s memory – storing everything from market data to order history to custom calculations.

The Cache serves multiple key purposes:

1. **Stores market data**:
   - Stores recent market history (e.g., order books, quotes, trades, bars).
   - Gives you access to both current and historical market data for your strategy.

2. **Tracks trading data**:
   - Maintains complete `Order` history and current execution state.
   - Tracks all `Position`s and `Account` information.
   - Stores `Instrument` definitions and `Currency` information.

3. **Stores custom data**:
   - You can store any user-defined objects or data in the `Cache` for later use.
   - Enables data sharing between different strategies.

## How caching works

**Built-in types**:

- The system automatically adds data to the `Cache` as it flows through.
- In live contexts, the engine applies updates asynchronously, so you might see a brief delay between an event and its appearance in the `Cache`.
- All data flows through the `Cache` before reaching your strategy’s callbacks – see the diagram below:

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐     ┌───────────────────────┐
│                 │     │                 │     │                 │     │                       │
│                 │     │                 │     │                 │     │   Strategy callback:  │
│      Data       ├─────►   DataEngine    ├─────►     Cache       ├─────►                       │
│                 │     │                 │     │                 │     │   on_data(...)        │
│                 │     │                 │     │                 │     │                       │
└─────────────────┘     └─────────────────┘     └─────────────────┘     └───────────────────────┘
```

### Basic example

Within a strategy, you can access the `Cache` through `self.cache`. Here’s a typical example:

:::note
Anywhere you find `self`, it refers mostly to the `Strategy` itself.
:::

```python
def on_bar(self, bar: Bar) -> None:
    # Current bar is provided in the parameter 'bar'

    # Get historical bars from Cache
    last_bar = self.cache.bar(self.bar_type, index=0)        # Last bar (practically the same as the 'bar' parameter)
    previous_bar = self.cache.bar(self.bar_type, index=1)    # Previous bar
    third_last_bar = self.cache.bar(self.bar_type, index=2)  # Third last bar

    # Get current position information
    if self.last_position_opened_id is not None:
        position = self.cache.position(self.last_position_opened_id)
        if position.is_open:
            # Check position details
            current_pnl = position.unrealized_pnl

    # Get all open orders for our instrument
    open_orders = self.cache.orders_open(instrument_id=self.instrument_id)
```

## Configuration

Use the `CacheConfig` class to configure the `Cache` behavior and capacity.
You can provide this configuration either to a `BacktestEngine` or a `TradingNode`, depending on your [environment context](architecture.md#environment-contexts).

Here's a basic example of configuring the `Cache`:

```python
from nautilus_trader.config import CacheConfig, BacktestEngineConfig, TradingNodeConfig

# For backtesting
engine_config = BacktestEngineConfig(
    cache=CacheConfig(
        tick_capacity=10_000,  # Store last 10,000 ticks per instrument
        bar_capacity=5_000,    # Store last 5,000 bars per bar type
    ),
)

# For live trading
node_config = TradingNodeConfig(
    cache=CacheConfig(
        tick_capacity=10_000,
        bar_capacity=5_000,
    ),
)
```

:::tip
By default, the `Cache` keeps the last 10,000 bars for each bar type and 10,000 trade ticks per instrument.
These limits provide a good balance between memory usage and data availability. Increase them if your strategy needs more historical data.
:::

### Configuration options

The `CacheConfig` class supports these parameters:

```python
from nautilus_trader.config import CacheConfig

cache_config = CacheConfig(
    database: DatabaseConfig | None = None,  # Database configuration for persistence
    encoding: str = "msgpack",               # Data encoding format ('msgpack' or 'json')
    timestamps_as_iso8601: bool = False,     # Store timestamps as ISO8601 strings
    buffer_interval_ms: int | None = None,   # Buffer interval for batch operations
    use_trader_prefix: bool = True,          # Use trader prefix in keys
    use_instance_id: bool = False,           # Include instance ID in keys
    flush_on_start: bool = False,            # Clear database on startup
    drop_instruments_on_reset: bool = True,  # Clear instruments on reset
    tick_capacity: int = 10_000,             # Maximum ticks stored per instrument
    bar_capacity: int = 10_000,              # Maximum bars stored per each bar-type
)
```

:::note
Each bar type maintains its own separate capacity. For example, if you're using both 1-minute and 5-minute bars, each stores up to `bar_capacity` bars.
When `bar_capacity` is reached, the `Cache` automatically removes the oldest data.
:::

### Database configuration

For persistence between system restarts, you can configure a database backend.

When is it useful to use persistence?

- **Long-running systems**: If you want your data to survive system restarts, upgrading, or unexpected failures, having a database configuration helps to pick up exactly where you left off.
- **Historical insights**: When you need to preserve past trading data for detailed post-analysis or audits.
- **Multi-node or distributed setups**: If multiple services or nodes need to access the same state, a persistent store helps ensure shared and consistent data.

```python
from nautilus_trader.config import DatabaseConfig

config = CacheConfig(
    database=DatabaseConfig(
        type="redis",      # Database type
        host="localhost",  # Database host
        port=6379,         # Database port
        timeout=2,         # Connection timeout (seconds)
    ),
)
```

## Using the cache

### Accessing market data

The `Cache` provides a comprehensive interface for accessing order books, quotes, trades, and bars.
All market data in the cache uses reverse indexing, so the most recent entry sits at index 0.

#### Bar access

```python
# Get a list of all cached bars for a bar type
bars = self.cache.bars(bar_type)  # Returns List[Bar] or an empty list if no bars found

# Get the most recent bar
latest_bar = self.cache.bar(bar_type)  # Returns Bar or None if no such object exists

# Get a specific historical bar by index (0 = most recent)
second_last_bar = self.cache.bar(bar_type, index=1)  # Returns Bar or None if no such object exists

# Check if bars exist and get count
bar_count = self.cache.bar_count(bar_type)  # Returns number of bars in cache for the specified bar type
has_bars = self.cache.has_bars(bar_type)    # Returns bool indicating if any bars exist for the specified bar type
```

#### Quote ticks

```python
# Get quotes
quotes = self.cache.quote_ticks(instrument_id)                     # Returns List[QuoteTick] or an empty list if no quotes found
latest_quote = self.cache.quote_tick(instrument_id)                # Returns QuoteTick or None if no such object exists
second_last_quote = self.cache.quote_tick(instrument_id, index=1)  # Returns QuoteTick or None if no such object exists

# Check quote availability
quote_count = self.cache.quote_tick_count(instrument_id)  # Returns the number of quotes in cache for this instrument
has_quotes = self.cache.has_quote_ticks(instrument_id)    # Returns bool indicating if any quotes exist for this instrument
```

#### Trade ticks

```python
# Get trades
trades = self.cache.trade_ticks(instrument_id)         # Returns List[TradeTick] or an empty list if no trades found
latest_trade = self.cache.trade_tick(instrument_id)    # Returns TradeTick or None if no such object exists
second_last_trade = self.cache.trade_tick(instrument_id, index=1)  # Returns TradeTick or None if no such object exists

# Check trade availability
trade_count = self.cache.trade_tick_count(instrument_id)  # Returns the number of trades in cache for this instrument
has_trades = self.cache.has_trade_ticks(instrument_id)    # Returns bool indicating if any trades exist
```

#### Order book

```python
# Get current order book
book = self.cache.order_book(instrument_id)  # Returns OrderBook or None if no such object exists

# Check if order book exists
has_book = self.cache.has_order_book(instrument_id)  # Returns bool indicating if an order book exists

# Get count of order book updates
update_count = self.cache.book_update_count(instrument_id)  # Returns the number of updates received
```

#### Price access

```python
from nautilus_trader.core.rust.model import PriceType

# Get current price by type; Returns Price or None.
price = self.cache.price(
    instrument_id=instrument_id,
    price_type=PriceType.MID,  # Options: BID, ASK, MID, LAST
)
```

#### Bar types

```python
from nautilus_trader.core.rust.model import PriceType, AggregationSource

# Get all available bar types for an instrument; Returns List[BarType].
bar_types = self.cache.bar_types(
    instrument_id=instrument_id,
    price_type=PriceType.LAST,  # Options: BID, ASK, MID, LAST
    aggregation_source=AggregationSource.EXTERNAL,
)
```

#### Simple example

```python
class MarketDataStrategy(Strategy):
    def on_start(self):
        # Subscribe to 1-minute bars
        self.bar_type = BarType.from_str(f"{self.instrument_id}-1-MINUTE-LAST-EXTERNAL")  # example of instrument_id = "EUR/USD.FXCM"
        self.subscribe_bars(self.bar_type)

    def on_bar(self, bar: Bar) -> None:
        bars = self.cache.bars(self.bar_type)[:3]
        if len(bars) < 3:   # Wait until we have at least 3 bars
            return

        # Access last 3 bars for analysis
        current_bar = bars[0]    # Most recent bar
        prev_bar = bars[1]       # Second to last bar
        prev_prev_bar = bars[2]  # Third to last bar

        # Get latest quote and trade
        latest_quote = self.cache.quote_tick(self.instrument_id)
        latest_trade = self.cache.trade_tick(self.instrument_id)

        if latest_quote is not None:
            current_spread = latest_quote.ask_price - latest_quote.bid_price
            self.log.info(f"Current spread: {current_spread}")
```

### Trading objects

The `Cache` provides comprehensive access to all trading objects within the system, including:

- Orders
- Positions
- Accounts
- Instruments

#### Orders

You can access and query orders through multiple methods, with flexible filtering options by venue, strategy, instrument, and order side.

##### Basic order access

```python
# Get a specific order by its client order ID
order = self.cache.order(ClientOrderId("O-123"))

# Get all orders in the system
orders = self.cache.orders()

# Get orders filtered by specific criteria
orders_for_venue = self.cache.orders(venue=venue)                       # All orders for a specific venue
orders_for_strategy = self.cache.orders(strategy_id=strategy_id)        # All orders for a specific strategy
orders_for_instrument = self.cache.orders(instrument_id=instrument_id)  # All orders for an instrument
```

##### Order state queries

```python
# Get orders by their current state
open_orders = self.cache.orders_open()          # Orders currently active at the venue
closed_orders = self.cache.orders_closed()      # Orders that have completed their lifecycle
emulated_orders = self.cache.orders_emulated()  # Orders being simulated locally by the system
inflight_orders = self.cache.orders_inflight()  # Orders submitted (or modified) to venue, but not yet confirmed

# Check specific order states
exists = self.cache.order_exists(client_order_id)            # Checks if an order with the given ID exists in the cache
is_open = self.cache.is_order_open(client_order_id)          # Checks if an order is currently open
is_closed = self.cache.is_order_closed(client_order_id)      # Checks if an order is closed
is_emulated = self.cache.is_order_emulated(client_order_id)  # Checks if an order is being simulated locally
is_inflight = self.cache.is_order_inflight(client_order_id)  # Checks if an order is submitted or modified, but not yet confirmed
```

##### Order statistics

```python
# Get counts of orders in different states
open_count = self.cache.orders_open_count()          # Number of open orders
closed_count = self.cache.orders_closed_count()      # Number of closed orders
emulated_count = self.cache.orders_emulated_count()  # Number of emulated orders
inflight_count = self.cache.orders_inflight_count()  # Number of inflight orders
total_count = self.cache.orders_total_count()        # Total number of orders in the system

# Get filtered order counts
buy_orders_count = self.cache.orders_open_count(side=OrderSide.BUY)  # Number of currently open BUY orders
venue_orders_count = self.cache.orders_total_count(venue=venue)      # Total number of orders for a given venue
```

#### Positions

The `Cache` maintains a record of all positions and offers several ways to query them.

##### Position access

```python
# Get a specific position by its ID
position = self.cache.position(PositionId("P-123"))

# Get positions by their state
all_positions = self.cache.positions()            # All positions in the system
open_positions = self.cache.positions_open()      # All currently open positions
closed_positions = self.cache.positions_closed()  # All closed positions

# Get positions filtered by various criteria
venue_positions = self.cache.positions(venue=venue)                       # Positions for a specific venue
instrument_positions = self.cache.positions(instrument_id=instrument_id)  # Positions for a specific instrument
strategy_positions = self.cache.positions(strategy_id=strategy_id)        # Positions for a specific strategy
long_positions = self.cache.positions(side=PositionSide.LONG)             # All long positions
```

##### Position state queries

```python
# Check position states
exists = self.cache.position_exists(position_id)        # Checks if a position with the given ID exists
is_open = self.cache.is_position_open(position_id)      # Checks if a position is open
is_closed = self.cache.is_position_closed(position_id)  # Checks if a position is closed

# Get position and order relationships
orders = self.cache.orders_for_position(position_id)       # All orders related to a specific position
position = self.cache.position_for_order(client_order_id)  # Find the position associated with a specific order
```

##### Position statistics

```python
# Get position counts in different states
open_count = self.cache.positions_open_count()      # Number of currently open positions
closed_count = self.cache.positions_closed_count()  # Number of closed positions
total_count = self.cache.positions_total_count()    # Total number of positions in the system

# Get filtered position counts
long_positions_count = self.cache.positions_open_count(side=PositionSide.LONG)              # Number of open long positions
instrument_positions_count = self.cache.positions_total_count(instrument_id=instrument_id)  # Number of positions for a given instrument
```

#### Accounts

```python
# Access account information
account = self.cache.account(account_id)       # Retrieve account by ID
account = self.cache.account_for_venue(venue)  # Retrieve account for a specific venue
account_id = self.cache.account_id(venue)      # Retrieve account ID for a venue
accounts = self.cache.accounts()               # Retrieve all accounts in the cache
```

#### Purging cached state

The cache exposes explicit maintenance hooks that remove closed or stale objects while preserving safety checks:

- `purge_closed_orders(ts_now, buffer_secs=0, purge_from_database=False)` drops closed orders that have been inactive for at least `buffer_secs`. Linked contingency orders remain until every dependent child is closed.
- `purge_closed_positions(ts_now, buffer_secs=0, purge_from_database=False)` removes positions that have stayed closed beyond the buffer window and deletes associated indices.
- `purge_account_events(ts_now, lookback_secs=0, purge_from_database=False)` trims account event history outside the lookback window and can cascade deletes to the backing database.

Key safeguards:

- Open orders and positions are never purged; the cache logs a warning and leaves the item intact.
- Linked orders keep parents in the cache until all children have closed, preventing premature removal of contingency chains.
- Indices and reverse lookups are cleaned alongside the primary object to avoid dangling references.
- Database deletions occur only when `purge_from_database=True` and a cache database is configured, ensuring in-memory purges do not silently erase persisted data.

Use the trading clock (for example, `self.clock.timestamp_ns()`) when supplying `ts_now`. Set `purge_from_database=True` only when you intend to delete persisted records from Redis or PostgreSQL as well. In live trading these methods run automatically when the execution engine is configured with purge intervals; see [Memory management](live.md#memory-management) for the scheduler settings.

#### Instruments and currencies

##### Instruments

```python
# Get instrument information
instrument = self.cache.instrument(instrument_id) # Retrieve a specific instrument by its ID
all_instruments = self.cache.instruments()        # Retrieve all instruments in the cache

# Get filtered instruments
venue_instruments = self.cache.instruments(venue=venue)              # Instruments for a specific venue
instruments_by_underlying = self.cache.instruments(underlying="ES")  # Instruments by underlying

# Get instrument identifiers
instrument_ids = self.cache.instrument_ids()                   # Get all instrument IDs
venue_instrument_ids = self.cache.instrument_ids(venue=venue)  # Get instrument IDs for a specific venue
```

##### Currencies

```python
# Get currency information
currency = self.cache.load_currency("USD")  # Loads currency data for USD
```

---

### Custom data

The `Cache` can also store and retrieve custom data types in addition to built-in market data and trading objects.
Use it to share any user-defined data between system components, primarily actors and strategies.

#### Basic storage and retrieval

```python
# Call this code inside Strategy methods (`self` refers to Strategy)

# Store data
self.cache.add(key="my_key", value=b"some binary data")

# Retrieve data
stored_data = self.cache.get("my_key")  # Returns bytes or None
```

For more complex use cases, the `Cache` can store custom data objects that inherit from the `nautilus_trader.core.Data` base class.

:::warning
The `Cache` is not designed to be a full database replacement. For large datasets or complex querying needs, consider using a dedicated database system.
:::

## Best practices and common questions

### Cache vs. portfolio usage

The `Cache` and `Portfolio` components serve different but complementary purposes in NautilusTrader:

**Cache**:

- Maintains the historical knowledge and current state of the trading system.
- Updates immediately when local state changes (for example, initializing an order before submission).
- Updates asynchronously as external events occur (for example, when an order fills).
- Provides a complete history of trading activity and market data.
- Keeps every event the strategy receives in the cache.

**Portfolio**:

- Aggregates position, exposure, and account information.
- Provides current state without history.

**Example**:

```python
class MyStrategy(Strategy):
    def on_position_changed(self, event: PositionEvent) -> None:
        # Use Cache when you need historical perspective
        position_history = self.cache.position_snapshots(event.position_id)

        # Use Portfolio when you need current real-time state
        current_exposure = self.portfolio.net_exposure(event.instrument_id)
```

### Cache vs. strategy variables

Choosing between storing data in the `Cache` versus strategy variables depends on your specific needs:

**Cache storage**:

- Use for data that needs to be shared between strategies.
- Best for data that needs to persist between system restarts.
- Acts as a central database accessible to all components.
- Ideal for state that needs to survive strategy resets.

**Strategy variables**:

- Use for strategy-specific calculations.
- Better for temporary values and intermediate results.
- Provides faster access and better encapsulation.
- Best for data that only your strategy needs.

**Example**:

The following example shows how you might store data in the `Cache` so multiple strategies can access the same information.

```python
import pickle

class MyStrategy(Strategy):
    def on_start(self):
        # Prepare data you want to share with other strategies
        shared_data = {
            "last_reset": self.clock.timestamp_ns(),
            "trading_enabled": True,
            # Include any other fields that you want other strategies to read
        }

        # Store it in the cache with a descriptive key
        # This way, multiple strategies can call self.cache.get("shared_strategy_info")
        # to retrieve the same data
        self.cache.add("shared_strategy_info", pickle.dumps(shared_data))

```

Another strategy can retrieve the cached data as follows:

```python
import pickle

class AnotherStrategy(Strategy):
    def on_start(self):
        # Load the shared data from the same key
        data_bytes = self.cache.get("shared_strategy_info")
        if data_bytes is not None:
            shared_data = pickle.loads(data_bytes)
            self.log.info(f"Shared data retrieved: {shared_data}")
```
