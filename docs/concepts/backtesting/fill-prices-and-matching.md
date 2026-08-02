# Fill Prices and Matching

The backtest matching engine treats recorded order book and trade data as immutable. Simulated fills
do not edit the historical book. This preserves the replayed market while requiring explicit
assumptions about whether the same displayed liquidity can fill more than one simulated order.

The engine provides two controls for those assumptions:

- `liquidity_consumption=True` tracks displayed size consumed at each price level.
- A fixed fill-model `random_seed` makes that model's probabilistic decisions repeatable.
  It does not configure randomness or execution ordering outside that model.

## Fill price determination

Fill prices depend on order type, liquidity side, book type, and the market state that caused the
match.

### L2 and L3 books

With depth data, market-style orders walk crossed book levels. A limit-style order receives crossed
book prices while acting as a taker and uses its limit price when acting as a maker.

| Order type             | Fill behavior                                                     |
| ---------------------- | ----------------------------------------------------------------- |
| `MARKET`               | Walks crossed book levels as a taker.                             |
| `MARKET_TO_LIMIT`      | Walks the book, then rests the remainder at its first fill price. |
| `LIMIT`                | Uses crossed levels as a taker or the limit price as a maker.     |
| `STOP_MARKET`          | Walks crossed levels after triggering.                            |
| `STOP_LIMIT`           | Uses the limit‑style rule after triggering.                       |
| `MARKET_IF_TOUCHED`    | Walks crossed levels after triggering.                            |
| `LIMIT_IF_TOUCHED`     | Uses the limit‑style rule after triggering.                       |
| `TRAILING_STOP_MARKET` | Walks crossed levels after activation and triggering.             |
| `TRAILING_STOP_LIMIT`  | Uses the limit‑style rule after activation and triggering.        |

A depth order can fill partially when the available crossed size is smaller than its remaining
quantity.

### L1 books

With an L1 book, the recorded market exposes only the best bid and ask:

| Order class                                                              | Fill behavior                                                                   |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------------- |
| `MARKET`, `MARKET_IF_TOUCHED`, `STOP_MARKET`, and `TRAILING_STOP_MARKET` | Use the market or trigger‑price rule, then fill any residual one tick worse.    |
| `MARKET_TO_LIMIT`                                                        | Uses the best opposite quote, then rests the remainder at the first fill price. |
| Limit‑style taker                                                        | Uses the best crossed quote, bounded by the limit price.                        |
| Limit‑style maker                                                        | Uses the limit price when matched by a trade or market move.                    |

The one-tick residual fill applies after the eligible market-style orders exhaust displayed L1
size. Price protection can prevent that residual fill if it would cross the configured boundary.
This deterministic residual rule is separate from probabilistic fill-model slippage.

Trade-driven matching has one additional rule. If a trade provides fill evidence at a price absent
from the book, the engine fills at the order's limit price and caps the quantity at the trade size.
See [trade-based execution](trade-execution.md#trade-driven-matching).

Fill models can change prices or provide a synthetic depth book. See
[fill models](fill-models.md).

### Triggered market-order fills with bars

Bar execution distinguishes a gap from an intrabar move for `STOP_MARKET`, `MARKET_IF_TOUCHED`, and
`TRAILING_STOP_MARKET` orders.

If the bar opens beyond the trigger, the order fills at the simulated market price. For example, a
SELL stop at 100 can fill at 90 when the next bar opens at 90.

If the bar opens before the trigger and a later high, low, or close moves through it, the engine
uses the trigger price. For example:

1. A SELL stop has a trigger at 100.
1. The bar opens at 102.
1. The low reaches 98.
1. The order fills at 100.

This rule assumes a continuous move through the trigger. Use quote, trade, or order book data when
the strategy requires more precise gap and path behavior.

## Price protection

Price protection limits how far `MARKET` and `STOP_MARKET` orders can walk the book. Configure the
offset as a number of instrument price increments:

```python
from nautilus_trader.backtest import BacktestVenueConfig
from nautilus_trader.model import AccountType
from nautilus_trader.model import BookType
from nautilus_trader.model import OmsType

venue = BacktestVenueConfig(
    name="SIM",
    oms_type=OmsType.NETTING,
    account_type=AccountType.MARGIN,
    book_type=BookType.L2_MBP,
    starting_balances=["100_000 USDT"],
    price_protection_points=100,
)
```

The engine calculates the boundary at fill time:

- BUY: `ask + (points * price_increment)`
- SELL: `bid - (points * price_increment)`

For an instrument with a 0.01 price increment, 100 points allow a BUY to fill at most 1.00 above
the current ask. Levels beyond the boundary are excluded, so the order can remain partially filled.

A market order gets its boundary when processed. A stop-market order gets its boundary when
triggered, using the bid or ask at that time. Set `price_protection_points=0` to disable protection.

## Order book immutability

A simulated fill never decrements the historical book. By default, each matching iteration can use
the full recorded size:

```python
from nautilus_trader.backtest import BacktestVenueConfig
from nautilus_trader.model import AccountType
from nautilus_trader.model import BookType
from nautilus_trader.model import OmsType

venue = BacktestVenueConfig(
    name="SIM",
    oms_type=OmsType.NETTING,
    account_type=AccountType.CASH,
    book_type=BookType.L1_MBP,
    starting_balances=["100_000 USD"],
    liquidity_consumption=True,
)
```

With `liquidity_consumption=True`, the engine records the original and consumed size for each price
level. Available size is `original_size - consumed`. A fresh book update at that level resets the
record to the new displayed size.

### L1 passive fills

When an L1 market moves through a passive limit:

| `liquidity_consumption` | Remaining‑quantity behavior                                           |
| ----------------------- | --------------------------------------------------------------------- |
| `False`                 | Fill the complete remaining order at its limit price.                 |
| `True`                  | Fill only the unconsumed displayed size and leave the remainder open. |

For example:

1. The ask is 100.10 for 50 units.
1. A BUY LIMIT for 1,000 units rests at 100.05.
1. The next ask is 100.00 for 30 units.
1. With consumption enabled, 30 units fill and 970 remain open.
1. Later updates can provide more fresh size.

### Trade consumption

A trade can provide executable size at a price absent from the current book. With consumption
enabled, trade-driven fills share that size instead of letting every order consume the full trade.

For L2 and L3 books, the triggering trade may already have consumed displayed depth. The engine
accounts for that volume before triggered orders use the remaining level. It skips this adjustment
when a newer book update already reflects the trade.

Synthetic books returned by a fill model do not use per-level consumption tracking. The model must
represent its own liquidity assumptions.

### Limitations

Consumption tracking estimates available size, not order priority. Set `queue_position=True` with
book and trade data for displayed-queue tracking, or use `prob_fill_on_limit` for a probabilistic
approximation.

Trade-driven fills are also opportunistic: a print proves that liquidity existed momentarily, not
that it remained available after the recorded trade.

## Precision requirements

Prices and quantities must use the instrument's configured `price_precision` and `size_precision`.
The outcome of a mismatch depends on where it enters the matching engine:

| Input            | Validated fields                                     | Mismatch outcome                                             |
| ---------------- | ---------------------------------------------------- | ------------------------------------------------------------ |
| `QuoteTick`      | Bid and ask prices and sizes                         | Log a warning and skip the tick.                             |
| `TradeTick`      | Price and size                                       | Log a warning and skip the tick.                             |
| Executable `Bar` | Open, high, low, close, and volume                   | Log a warning and skip the bar.                              |
| New order        | Quantity, display quantity, price, and trigger price | Reject the order.                                            |
| Order update     | Quantity, price, and trigger price                   | Reject the modification.                                     |
| Generated fill   | Fill price and quantity                              | Normalize when compatible; otherwise warn and skip the fill. |

The engine logs an error after 20 consecutive market-data precision mismatches. With
`shutdown_on_error=True`, that error can request a normal backtest shutdown. A valid quote, trade,
or executable bar resets the consecutive-mismatch count.

`Bar.volume` must use the instrument's quantity units and size precision. Convert provider-specific
quote-volume fields before creating the bar.

Use the instrument factories to construct compatible values:

```python
price = instrument.make_price(raw_price)
quantity = instrument.make_qty(raw_quantity)
```

Also verify that the instrument definition matches the data source and that custom loaders preserve
the source precision.
