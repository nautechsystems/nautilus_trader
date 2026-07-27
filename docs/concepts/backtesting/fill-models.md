# Fill Models

Historical data cannot show how a simulated order would have interacted with other market
participants. A fill model controls the assumptions NautilusTrader makes about limit-order
eligibility, one-tick slippage, and optional synthetic liquidity.

## Behavior by book type

With L2 or L3 data, the recorded book supplies price levels and sizes. The matching engine walks
those levels, and `prob_fill_on_limit` can model whether a touched limit order fills.
`prob_slippage` does not apply because the book itself determines price impact.

With an L1 book, including books updated from quotes, trades, or bars:

- `prob_fill_on_limit` controls whether a limit order fills when its price is touched.
- `prob_slippage` is evaluated for every fill, regardless of order type or liquidity side.
- A successful slippage draw moves the fill one tick against the order direction.
- A model may provide a synthetic L2 book to represent liquidity beyond the best bid and ask.

For example, with `prob_slippage=0.5`, each BUY fill has a 50% chance of moving one tick higher.
Set `random_seed` when a run must reproduce the model's random draws.

If a venue does not specify a fill model, it uses `DefaultFillModel` with
`prob_fill_on_limit=1.0` and `prob_slippage=0.0`. The model therefore considers a touched limit
fill-eligible, and L1 fills do not receive probabilistic one-tick slippage by default. This does
not disable the matching engine's separate residual-fill rule for eligible market-style orders.

:::warning
Historical order book data remains immutable after a fill. With
`liquidity_consumption=False`, the same displayed size can support more than one simulated order in
an iteration. Set `liquidity_consumption=True` to track consumed size per level until fresh data
arrives. See [order book immutability](fill-prices-and-matching.md#order-book-immutability).
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
| `MarketHoursFillModel`       | Uses a normal or one‑tick‑wider synthetic spread.       |

The tier sizes are model constants expressed in instrument quantity units. Confirm that they suit
the scale of the instrument before using a tiered model.

The current Python bindings do not expose the state setters for `VolumeSensitiveFillModel` or
`MarketHoursFillModel`. From Python, they retain their initial values of 1,000 recent-volume units
and normal-liquidity mode.

## Configuration

Pass a built-in model object directly to `BacktestVenueConfig`:

```python
from nautilus_trader.backtest import BacktestVenueConfig
from nautilus_trader.execution import DefaultFillModel
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
)
```

Synthetic book models use the same constructor parameters:

```python
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
)
```

The current high-level venue configuration accepts built-in fill models. It does not load fill
models from import-path configuration objects.

The low-level `BacktestEngine.add_venue()` method also accepts a custom Python object. It must
implement:

- `is_limit_filled() -> bool`
- `is_slipped() -> bool`

It may also implement:

- `fill_limit_inside_spread() -> bool`
- `get_orderbook_for_fill_simulation(instrument, order, best_bid, best_ask) -> OrderBook | None`

Subclassing `nautilus_trader.execution.FillModel` supplies default implementations for these
methods. This custom-object protocol applies to the low-level engine only.

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
- `1.0`: Add one tick to every fill.

The draw applies to maker and taker fills. It does not apply to L2 or L3 books.

## Synthetic order books

Before determining a fill, the matching engine asks the model for an optional synthetic order book.
If the model returns a book, the engine fills against its levels. If it returns `None`, the engine
uses the recorded book.

Per-level `liquidity_consumption` tracking does not apply to a synthetic model book. A custom model
must represent any desired consumption behavior in the books it returns.
