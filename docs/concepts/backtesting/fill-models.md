# Fill Models

Historical data cannot show how a simulated order would have interacted with other market
participants. A **fill model** controls the assumptions NautilusTrader makes about limit-order
eligibility, one-tick slippage, and optional synthetic liquidity.

## Behavior by book type

With L2 or L3 data, the recorded book supplies price levels and sizes. The matching engine walks
those levels, and `prob_fill_on_limit` can model whether a touched limit order fills.
`prob_slippage` does not apply because the book itself determines price impact.

With an L1 book, including books updated from quotes, trades, or bars:

- `prob_fill_on_limit` controls whether a limit order fills when its price is touched.
- `prob_slippage` is evaluated for each fill, regardless of order type or liquidity side.
- A successful slippage draw moves the fill one tick against the order direction.
- A model may provide a synthetic L2 book to represent liquidity beyond the best bid and ask.

For example, with `prob_slippage=0.5`, each BUY fill has a 50% chance of moving one tick higher.
Set `random_seed` when a run must reproduce the model's random draws.

If a venue does not specify a fill model, it uses `DefaultFillModel` with:

- `prob_fill_on_limit=1.0`: A touched limit order is fill-eligible.
- `prob_slippage=0.0`: L1 fills receive no probabilistic one-tick slippage.

These defaults do not disable the matching engine's separate residual-fill rule for eligible
market-style orders.

:::warning
Historical order book data stays immutable after a fill. With `liquidity_consumption=False`, the same
displayed size can support more than one simulated order in an iteration. Set `liquidity_consumption=True` to
track consumed size per level until fresh data arrives. See
[order book immutability](fill-prices-and-matching.md#order-book-immutability).
:::

## Available models

| Model                        | Liquidity behavior                                      |
| ---------------------------- | ------------------------------------------------------- |
| `DefaultFillModel`           | Uses the matching engine's recorded book.               |
| `BestPriceFillModel`         | Provides unlimited size at the best bid and ask.        |
| `OneTickSlippageFillModel`   | Provides unlimited size one tick beyond the best price. |
| `ProbabilisticFillModel`     | Chooses the best price or one tick worse.               |
| `TwoTierFillModel`           | Places 10 units at best, then the rest one tick worse.  |
| `ThreeTierFillModel`         | Places 50, 30, and 20 units across three levels.        |
| `LimitOrderPartialFillModel` | Places 5 units at best, then the rest one tick worse.   |
| `SizeAwareFillModel`         | Changes the book shape at an order size of 10 units.    |
| `CompetitionAwareFillModel`  | Exposes a configurable fraction of 1,000 units at best. |
| `VolumeSensitiveFillModel`   | Places 25% of its internal volume at best.              |
| `MarketHoursFillModel`       | Uses a normal or one-tick-wider synthetic spread.       |

`BestPriceFillModel` also fills a limit order at or inside the spread at the order's own price.

**Instrument scale**: Tier sizes are model constants expressed in instrument quantity units.
Confirm that they suit the instrument's scale before using a tiered model.

Synthetic books omit any level outside the instrument's `min_price` and `max_price`. They also omit zero or
negative prices when the instrument does not allow them.

`CompetitionAwareFillModel` accepts `liquidity_factor` values in `[0.0, 1.0]`, defaults to `0.3`,
and clamps the calculated size to at least one instrument quantity unit.

**Python state**: The bindings do not expose the state setters for `VolumeSensitiveFillModel` or
`MarketHoursFillModel`. From Python, these models keep their initial values of 1,000 recent-volume units and
normal-liquidity mode, respectively.

## Configuration

Pass a built-in or custom Python model object directly to `BacktestVenueConfig`:

```python
from decimal import Decimal

from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.execution import DefaultFillModel
from nautilus_trader.execution import MakerTakerFeeModel
from nautilus_trader.model import AccountType
from nautilus_trader.model import BookType
from nautilus_trader.model import OmsType

venue = BacktestVenueConfig(
    name="SIM",
    oms_type=OmsType.NETTING,
    account_type=AccountType.CASH,
    book_type=BookType.L1_MBP,
    starting_balances=["100_000 USD"],
    fill_model=DefaultFillModel(
        prob_fill_on_limit=0.2,
        prob_slippage=0.5,
        random_seed=42,
    ),
    fee_model=MakerTakerFeeModel(
        maker_rate=Decimal("0"),
        taker_rate=Decimal("0"),
    ),
)
```

Synthetic book models use the same constructor parameters:

```python
from decimal import Decimal

from nautilus_trader.execution import MakerTakerFeeModel
from nautilus_trader.execution import ThreeTierFillModel

venue = BacktestVenueConfig(
    name="SIM",
    oms_type=OmsType.NETTING,
    account_type=AccountType.CASH,
    book_type=BookType.L1_MBP,
    starting_balances=["100_000 USD"],
    fill_model=ThreeTierFillModel(
        prob_fill_on_limit=1.0,
        prob_slippage=0.0,
        random_seed=42,
    ),
    fee_model=MakerTakerFeeModel(
        maker_rate=Decimal("0"),
        taker_rate=Decimal("0"),
    ),
)
```

The high-level venue configuration accepts model objects directly. It does not load fill models
from import-path configuration objects.

### Custom fill models

Pass a custom Python object as `fill_model` to `BacktestVenueConfig`, `BacktestEngine.add_venue()`,
`BacktestEngine.change_fill_model(venue, fill_model)`, or `SandboxExecutionClientConfig`. It must implement:

- `is_limit_filled() -> bool`
- `is_slipped() -> bool`

It may also implement:

- `fill_limit_inside_spread() -> bool`
- `get_orderbook_for_fill_simulation(instrument, order, best_bid, best_ask) -> OrderBook | None`

Subclassing `nautilus_trader.execution.FillModel` supplies default implementations for these
methods: limit fills return `True`, slippage and spread fills return `False`, and the liquidity hook
returns `None`. Compatible objects do not need to inherit `FillModel`; omitted optional methods use
these same defaults. Objects missing either required method raise `TypeError` during configuration.

Configuration getters and copies retain the supplied Python instance. Callback state is shared with
runtime copies and persists across backtest runs and resets. Use a fresh model for independent state.
Callback failures propagate from the Python bridge as errors. Matching logs the error and stops that
fill attempt; fills already applied in the attempt remain. A failure in `fill_limit_inside_spread()`
logs the error and disables spread fills. The engine reads this hook when each matching engine is
created or its fill model is replaced, not for each order.

The liquidity hook receives `None` for a missing historical bid or ask. Custom models must handle
these optional prices. The return value determines the fill behavior:

- `None` uses the standard fill logic.
- An `OrderBook` restricts fills to that book's eligible liquidity, even when no fills are available.
  Partial custom fills are not topped up with historical liquidity or the L1 remainder-fill rule.

## Probabilistic parameters

### `prob_fill_on_limit` (default: `1.0`)

This value controls whether a limit order fills when the market touches, but does not cross, its
price:

- `0.0`: Never fill on touch.
- `0.5`: Fill on half of eligible touches on average.
- `1.0`: Always fill on touch.

Crossing the limit price is a separate matching condition. For explicit queue-volume tracking, see
[queue position tracking](trade-execution.md#queue-position-tracking).

### `prob_slippage` (default: `0.0`)

For L1 books, this value controls a one-tick adverse move on each fill:

- `0.0`: Never add model slippage.
- `0.5`: Add one tick on half of fills on average.
- `1.0`: Add one tick to each fill.

The draw applies to maker and taker fills. It does not apply to L2 or L3 books. A slip keeps the original fill
price if it would cross the instrument's `min_price` or `max_price`. It also keeps that price if it would
reach a zero or negative price that the instrument does not allow.

## Synthetic order books

Before determining a fill, the matching engine asks the model for an optional synthetic order book.
If the model returns a book, the engine fills against its levels. If it returns `None`, the engine
uses the recorded book.

:::warning[Synthetic book consumption]
Per-level `liquidity_consumption` tracking does not apply to a synthetic model book. A custom model
must represent any desired consumption behavior in the books it returns.
:::
