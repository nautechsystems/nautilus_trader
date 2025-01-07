# Backtesting

:::info
We are currently working on this guide.
:::

Backtesting with NautilusTrader is a methodical simulation process that replicates trading
activities using a specific system implementation. This system is composed of various components
including the built-in engines, `Cache`, [MessageBus](message_bus.md), `Portfolio`, [Actors](advanced/actors.md), [Strategies](strategies.md), [Execution Algorithms](execution.md),
and other user-defined modules. The entire trading simulation is predicated on a stream of historical data processed by a
`BacktestEngine`. Once this data stream is exhausted, the engine concludes its operation, producing
detailed results and performance metrics for in-depth analysis.

It's important to recognize that NautilusTrader offers two distinct API levels for setting up and conducting backtests:

- **High-level API**: Uses a `BacktestNode` and configuration objects (`BacktestEngine`s are used internally).
- **Low-level API**: Uses a `BacktestEngine` directly with more "manual" setup.

## Choosing an API level

Consider using the **low-level** API when:

- Your entire data stream can be processed within the available machine resources (e.g., RAM).
- You prefer not to store data in the Nautilus-specific Parquet format.
- You have a specific need or preference to retain raw data in its original format (e.g., CSV, binary, etc.).
- You require fine-grained control over the `BacktestEngine`, such as the ability to re-run backtests on identical datasets while swapping out components (e.g., actors or strategies) or adjusting parameter configurations.

Consider using the **high-level** API when:

- Your data stream exceeds available memory, requiring streaming data in batches.
- You want to leverage the performance and convenience of the `ParquetDataCatalog` for storing data in the Nautilus-specific Parquet format.
- You value the flexibility and functionality of passing configuration objects to define and manage multiple backtest runs across various engines simultaneously.

## Low-level API

The low-level API centers around a `BacktestEngine`, where inputs are initialized and added manually via a Python script.
An instantiated `BacktestEngine` can accept the following:

- Lists of `Data` objects, which are automatically sorted into monotonic order based on `ts_init`.
- Multiple venues, manually initialized.
- Multiple actors, manually initialized and added.
- Multiple execution algorithms, manually initialized and added.

This approach offers detailed control over the backtesting process, allowing you to manually configure each component.

## High-level API

The high-level API centers around a `BacktestNode`, which orchestrates the management of multiple `BacktestEngine` instances,
each defined by a `BacktestRunConfig`. Multiple configurations can be bundled into a list and processed by the node in one run.

Each `BacktestRunConfig` object consists of the following:

- A list of `BacktestDataConfig` objects.
- A list of `BacktestVenueConfig` objects.
- A list of `ImportableActorConfig` objects.
- A list of `ImportableStrategyConfig` objects.
- A list of `ImportableExecAlgorithmConfig` objects.
- An optional `ImportableControllerConfig` object.
- An optional `BacktestEngineConfig` object, with a default configuration if not specified.

## Data

Data provided for backtesting drives the execution flow. Since a variety of data types can be used,
it's crucial that your venue configurations align with the data being provided for backtesting.
Mismatches between data and configuration can lead to unexpected behavior during execution.

:::info
NautilusTrader is primarily designed and optimized for backtesting on order book or tick data, providing the highest
execution granularity and realism. If order book or tick data is unavailable or unsuitable, backtests
can also be run on bar data; however, users should note that this results in a loss of information and detail,
reducing execution precision and realism.
:::

### Bar based execution

NautilusTrader is primarily designed and optimized for order book and tick level data, which shows every price level
and order - just like what you'd see on a real exchange. This gives you the highest level of execution granularity
and realism, as the platform can process market data in descending order of detail:

1. L3 (market-by-order) and L2 (market-by-price) orderbook data - showing complete market depth and order flow.
2. L1 orderbook data - showing only the top of book with best bid and ask prices.
3. Quote ticks - providing current best bid and ask prices.
4. Trade ticks - showing actual executed trades.
5. Bars - summarizing trading activity over fixed time intervals.

#### Understanding bar data limitations

Bar data provides a summary of market activity with four key prices for each time period:

- Open: opening price (first trade).
- High: highest price reached.
- Low: lowest price hit.
- Close: closing price (last trade).

While this gives us a good overview of price movement, we lose some important details that we'd have with more granular data:

- We don't know if the market hit the high price before the low price.
- We can't see exactly when prices changed within the time period.
- We don't know the actual sequence of trades that occurred.

This is why NautilusTrader processes bar data through a sophisticated system that tries to maintain realistic market behavior despite these limitations. At its core, the platform always maintains a Level 2 (L2) order book simulation - even when you provide less granular data like bars, trades, quotes, or Level 1 (L1) order book updates. Think of it as the platform automatically "upgrading" simpler market data into a more detailed market structure with some limitations.

#### Processing bar data: Time / Prices / Executions

Even when you provide bar data, NautilusTrader maintains an internal order book for each instrument - just like a real exchange would.

1. **Time Processing**
   - NautilusTrader has a specific way of handling the timing of bar data that's crucial for accurate simulation.
   - Bar timestamps (`ts_event`) are expected to represent the close time of the bar. This approach is most logical because it represents the moment when the bar is fully formed and its aggregation is complete.
   - The initialization time (`ts_init`) can be controlled using the `ts_init_delta` parameter in `BarDataWrangler`, which should typically be set to the bar's step size (timeframe) in nanoseconds.
   - The platform ensures all events happen in the correct sequence based on these timestamps, preventing any possibility of look-ahead bias in your backtests.
2. **Price Processing**
   - The platform converts each bar's OHLC prices into a sequence of market updates.
   - These updates always follow the same order: Open → High → Low → Close.
   - If you provide multiple timeframes (like both 1-minute and 5-minute bars), the platform smartly uses the more detailed data for better accuracy.
3. **Executions**
   - When you place any orders, they interact with the simulated order book just like they would on a real exchange.
   - For market orders, execution happens at the current simulated market price plus any configured latency.
   - For limit orders sitting in the market, they'll execute if any of the bar's prices cross your limit price.
   - The matching engine continuously processes orders as prices move, rather than waiting for complete bars.




### Slippage and Spread Handling

When backtesting with different types of data, NautilusTrader implements specific handling for slippage and spread simulation:

For L2 (market-by-price) or L3 (market-by-order) data, slippage is simulated with high accuracy by:
- Filling orders against actual order book levels
- Matching available size at each price level sequentially
- Maintaining realistic order book depth impact (per order fill)

For L1 data types (e.g., L1 orderbook, trades, quotes, bars), slippage is handled through:

**Initial Fill Slippage** (`prob_slippage`)
   - Controlled by the FillModel's `prob_slippage` parameter.
   - Determines if the initial fill will occur one tick away from current market price.
   - Example: With `prob_slippage=0.5`, a market buy has 50% chance to fill one tick above best ask.

:::note
When backtesting with bar data, be aware that the reduced granularity of price information affects the slippage mechanism.
For the most realistic backtesting results, consider using higher granularity data sources such as L2 or L3 order book data when available.
:::

### Fill Model

The `FillModel` helps simulate probabilistic aspects of order execution during backtesting. It addresses a fundamental
challenge: even with perfect historical market data, we can't fully simulate how orders would have interacted with other
market participants in real-time.

The FillModel simulates 2 key aspects of trading that exist in real markets regardless of data quality:

1. **Queue Position for Limit Orders**
   - When multiple traders place orders at the same price level, the order's position in the queue affects if and when it gets filled.
2. **Market Impact and Competition**
   - When taking liquidity with market orders, you compete with other traders for available liquidity, which can affect your fill price.

#### Configuration and Parameters

```python
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.engine import BacktestEngine, BacktestEngineConfig

# Create a custom fill model with your desired probabilities
fill_model = FillModel(
    prob_fill_on_limit=0.2,    # Chance a limit order fills when price matches (applied to bars/trades/quotes + L1/L2/L3 orderbook)
    prob_fill_on_stop=0.95,    # [DEPRECATED] Will be removed in future versions, use `prob_slippage` instead
    prob_slippage=0.5,         # Chance of 1-tick slippage (applied to bars/trades/quotes + L1 orderbook only)
    random_seed=None,          # Optional: Set for reproducible results
)

# Add the fill model to your engine configuration
engine = BacktestEngine(
    config=BacktestEngineConfig(
        trader_id="TESTER-001",
        fill_model=fill_model,  # Inject your custom fill model here
    )
)
```

**prob_fill_on_limit** (default: `1.0`)
- Purpose:
   - Simulates the probability of a limit order getting filled when its price level is reached in the market.
- Details:
   - Represents your position in the order queue at a given price level.
   - Applies to all data types (L3/L2/L1 orderbooks / trades / quotes / bars data).
   - New random probability check occurs each time market price touches your order price.
   - On successful probability check, fills entire remaining order quantity.
- Examples:
   - With `prob_fill_on_limit=0.0`:
      - Limit buy orders never fill when best ask reaches the limit price
      - Limit sell orders never fill when best bid reaches the limit price
      - This simulates being at the very back of the queue and never reaching the front
   - With `prob_fill_on_limit=0.5`:
      - Limit buy orders have 50% chance of filling when best ask reaches the limit price
      - Limit sell orders have 50% chance of filling when best bid reaches the limit price
      - This simulates being in the middle of the queue
   - With `prob_fill_on_limit=1.0` (default):
      - Limit buy orders always fill when best ask reaches the limit price
      - Limit sell orders always fill when best bid reaches the limit price
      - This simulates being at the front of the queue with guaranteed fills

**prob_slippage** (default: `0.0`)
- Purpose:
   - Simulates the probability of experiencing price slippage when executing market orders.
- Details:
     - Only applies to L1 data types (bars/trades/quotes/L1 orderbook).
     - When triggered, moves fill price one tick against your order direction.
     - Affects all market-type orders (MARKET, MARKET_TO_LIMIT, STOP_MARKET).
     - Not utilized with L2/L3 data where real order book depth determines slippage.
- Example:
   - With `prob_slippage=0.0` (default):
      - No artificial slippage is applied, representing an idealized scenario where you always get filled at the current market price
   - With `prob_slippage=0.5`:
      - Market buy orders have 50% chance of filling one tick above the best ask price, and 50% chance at the best ask price
      - Market sell orders have 50% chance of filling one tick below the best bid price, and 50% chance at the best bid price
   - With `prob_slippage=1.0`:
      - Market buy orders always fill one tick above the best ask price
      - Market sell orders always fill one tick below the best bid price
      - This simulates consistent adverse price movement against your orders

**prob_fill_on_stop** (default: `1.0`)
- **DEPRECATED**: This parameter will be removed in future versions.
- Stop order is just shorter name for stop-market order, that convert to market orders when market-price touches the stop-price
- That means, stop order order-fill mechanics is simply market-order mechanics, that is controlled by the `prob_slippage` parameter.

#### How Simulation Varies by Data Type

The FillModel's behavior adapts based on the order book type being used:

**L2/L3 Orderbook data**

With full order book depth, the FillModel focuses purely on simulating queue position for limit orders through `prob_fill_on_limit`. The order book itself handles slippage naturally based on available liquidity at each price level.

- `prob_fill_on_limit` is active - simulates queue position
- `prob_slippage` is not used - real order book depth determines price impact

**L1 Orderbook data**

With only best bid/ask prices available, the FillModel provides additional simulation:

- `prob_fill_on_limit` is active - simulates queue position
- `prob_slippage` is active - simulates basic price impact since we lack real depth information

**Bar/Quote/Trade data**

When using less granular data, the same behaviors apply as L1:

- `prob_fill_on_limit` is active - simulates queue position
- `prob_slippage` is active - simulates basic price impact

#### Important Considerations

The FillModel has certain limitations to keep in mind:

- Partial fills are not simulated - orders either fill completely or not at all
- With L1 data, slippage is limited to fixed 1 tick, at which entire order's quantity is filled

:::note
The FillModel continues to evolve. Future versions may introduce more sophisticated simulation of order execution dynamics, including:
- Partial fill simulation
- Variable slippage based on order size
- More complex queue position modeling
:::

## Venues

When initializing a venue for backtesting, you must specify its internal order `book_type` for execution processing from the following options:

- `L1_MBP`: Level 1 market-by-price (default). Only the top level of the order book is maintained.
- `L2_MBP`: Level 2 market-by-price. Order book depth is maintained, with a single order aggregated per price level.
- `L3_MBO`: Level 3 market-by-order. Order book depth is maintained, with all individual orders tracked as provided by the data.

:::note
The granularity of the data must match the specified order `book_type`. Nautilus cannot generate higher granularity data (L2 or L3) from lower-level data such as quotes, trades, or bars.
:::

:::warning
If you specify `L2_MBP` or `L3_MBO` as the venue’s `book_type`, all non-order book data (such as quotes, trades, and bars) will be ignored for execution processing.
This may cause orders to appear as though they are never filled. We are actively working on improved validation logic to prevent configuration and data mismatches.
:::

:::warning
When providing Level 2 or higher order book data, ensure that the `book_type` is updated to reflect the data's granularity.
Failing to do so will result in data aggregation: L2 data will be reduced to a single order per level, and L1 data will reflect only top-of-book levels.
:::
