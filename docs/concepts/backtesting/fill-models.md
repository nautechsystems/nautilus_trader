# Fill models

Fill models simulate order execution dynamics during backtesting. They address a fundamental
challenge: *even with perfect historical market data, we can't fully simulate how orders may have
interacted with other market participants in real-time*.

The base `FillModel` provides probabilistic parameters for queue position and slippage simulation.
Subclasses can override `get_orderbook_for_fill_simulation()` to generate synthetic order books for
more sophisticated liquidity modeling.

## Slippage and spread handling

When backtesting with different types of data, Nautilus implements specific handling for slippage
and spread simulation:

For L2 (market-by-price) or L3 (market-by-order) data, slippage is simulated with high accuracy by:

- Filling orders against actual order book levels.
- Matching available size at each price level sequentially.
- Maintaining realistic order book depth impact (per order fill).

For L1 data types (e.g., L1 order book, trades, quotes, bars), slippage is handled through the
`FillModel`:

**Per-fill slippage** (`prob_slippage`):

- Applies to each fill when using an L1 book with a configured `FillModel`.
- Affects all order types (market, limit, stop, etc.).
- When triggered, moves the fill price one tick against the order direction.
- Example: With `prob_slippage=0.5`, a BUY order has 50% chance of filling one tick above the best
  ask.

:::note
When backtesting with bar data, be aware that the reduced granularity of price information affects
the slippage mechanism. For the most realistic backtesting results, consider using higher
granularity data sources such as L2 or L3 order book data when available.
:::

## How simulation varies by data type

The behavior of the `FillModel` adapts based on the order book type being used:

**L2/L3 order book data**

With full order book depth, the `FillModel` focuses purely on simulating queue position for limit
orders through `prob_fill_on_limit`. The order book itself handles slippage naturally based on
available liquidity at each price level.

- `prob_fill_on_limit` is active - simulates queue position.
- `prob_slippage` is not used - real order book depth determines price impact.

:::warning
The historical order book is immutable during backtesting. Book depth is **not** decremented after
fills. By default (`liquidity_consumption=False`), the same liquidity can be consumed repeatedly
within an iteration. Enable `liquidity_consumption=True` to track consumed liquidity per price
level. Consumption resets when fresh data arrives at that level. See
[order book immutability](fill-prices-and-matching.md#order-book-immutability) for details.
:::

**L1 order book data**

With only best bid/ask prices available, the `FillModel` provides additional simulation:

- `prob_fill_on_limit` is active - simulates queue position.
- `prob_slippage` is active - simulates basic price impact since we lack real depth information.

**Bar/Quote/Trade data**

When using less granular data, the same behaviors apply as L1:

- `prob_fill_on_limit` is active - simulates queue position.
- `prob_slippage` is active - simulates basic price impact.

## Important considerations

- **Partial fills**: With L2/L3 data, fills are limited to available liquidity at each price level.
  With L1 data, the full order quantity fills at the single available level.
- **Consumption tracking**: See
  [order book immutability](fill-prices-and-matching.md#order-book-immutability) for details on
  preventing duplicate fills.

## Available fill models

| Model                        | Description                                             | Use case                                     |
| ---------------------------- | ------------------------------------------------------- | -------------------------------------------- |
| `FillModel`                  | Base model with probabilistic fill/slippage parameters. | Simple queue position and slippage.          |
| `BestPriceFillModel`         | Fills at best price with unlimited liquidity.           | Testing basic strategy logic optimistically. |
| `OneTickSlippageFillModel`   | Forces exactly one tick of slippage on all orders.      | Conservative slippage testing.               |
| `TwoTierFillModel`           | 10 contracts at best price, remainder one tick worse.   | Basic market depth simulation.               |
| `ThreeTierFillModel`         | 50/30/20 contracts across three price levels.           | More realistic depth simulation.             |
| `ProbabilisticFillModel`     | 50% chance best price, 50% chance one tick slippage.    | Randomized execution quality.                |
| `SizeAwareFillModel`         | Different execution based on order size (≤10 vs >10).   | Size‑dependent market impact.                |
| `LimitOrderPartialFillModel` | Max 5 contracts fill per price touch.                   | Queue position via partial fills.            |
| `MarketHoursFillModel`       | Wider spreads during low liquidity periods.             | Session‑aware execution.                     |
| `VolumeSensitiveFillModel`   | Liquidity based on recent trading volume.               | Volume‑adaptive depth.                       |
| `CompetitionAwareFillModel`  | Only percentage of visible liquidity available.         | Multi‑participant competition.               |

## Configuring fill models

**Using the base FillModel with probabilistic parameters:**

```python
from nautilus_trader.backtest.config import BacktestVenueConfig
from nautilus_trader.backtest.config import ImportableFillModelConfig

venue_config = BacktestVenueConfig(
    name="SIM",
    oms_type="NETTING",
    account_type="CASH",
    starting_balances=["100_000 USD"],
    fill_model=ImportableFillModelConfig(
        fill_model_path="nautilus_trader.backtest.models:FillModel",
        config_path="nautilus_trader.backtest.config:FillModelConfig",
        config={
            "prob_fill_on_limit": 0.2,  # Chance a limit order fills when price matches
            "prob_slippage": 0.5,  # Chance of 1-tick slippage (L1 data only)
            "random_seed": 42,  # Optional: Set for reproducible results
        },
    ),
)
```

**Using an order book simulation model:**

```python
from nautilus_trader.backtest.config import BacktestVenueConfig
from nautilus_trader.backtest.config import ImportableFillModelConfig

venue_config = BacktestVenueConfig(
    name="SIM",
    oms_type="NETTING",
    account_type="CASH",
    starting_balances=["100_000 USD"],
    fill_model=ImportableFillModelConfig(
        fill_model_path="nautilus_trader.backtest.models:ThreeTierFillModel",
    ),
)
```

## Probabilistic parameters

**prob_fill_on_limit** (default: `1.0`)

Simulates queue position by controlling the probability of a limit order filling when its price
level is touched (but not crossed).

- `0.0`: Never fills at touch (back of queue).
- `0.5`: 50% chance of filling (middle of queue).
- `1.0`: Always fills at touch (front of queue).

**prob_slippage** (default: `0.0`)

Simulates price slippage on each fill. Only applies to L1 data types (quotes, trades, bars) where
real depth is unavailable. Affects all order types when executing as takers.

- `0.0`: No slippage (fills at best price).
- `0.5`: 50% chance of one tick slippage per fill.
- `1.0`: Always slips one tick.

## Order book simulation models

These models override the `get_orderbook_for_fill_simulation()` method to generate synthetic order
books representing expected market liquidity. The matching engine fills orders against this
simulated book.

**How it works:**

1. Before processing a fill, the matching engine calls `get_orderbook_for_fill_simulation()`.
2. If the model returns a synthetic order book, fills execute against that book's liquidity.
3. If the model returns `None`, standard fill logic applies.

:::note
When a custom fill model provides a simulated order book, the `liquidity_consumption` tracking is
**not** applied. Custom fill models are expected to manage their own liquidity simulation within the
returned order book. Liquidity consumption tracking only affects the built-in fill logic (when
`get_orderbook_for_fill_simulation()` returns `None`).
:::

**Example: ThreeTierFillModel**

This model creates a book with liquidity distributed across three price levels:

- 50 contracts at best price
- 30 contracts one tick worse
- 20 contracts two ticks worse

A 100-contract market order would fill partially at each level, experiencing realistic price impact.

**Creating custom fill models:**

```python
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.model.book import OrderBook, BookOrder
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.core.rust.model import BookType


class MyCustomFillModel(FillModel):
    def get_orderbook_for_fill_simulation(
        self,
        instrument,
        order,
        best_bid,
        best_ask,
    ):
        book = OrderBook(
            instrument_id=instrument.id,
            book_type=BookType.L2_MBP,
        )

        # Add custom liquidity based on your market model
        # ...

        return book
```
