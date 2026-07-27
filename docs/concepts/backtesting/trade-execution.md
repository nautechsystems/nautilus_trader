# Trade-Based Execution

Trade ticks trigger matching by default when a venue has `trade_execution=True`. A trade provides
evidence that liquidity traded at its price, so it can fill resting orders on the passive side.

Set `trade_execution=False` to use trades as strategy data without letting them trigger matching:

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
    trade_execution=False,
)
```

When trade execution is disabled, trade ticks do not run order matching or matching-engine
maintenance such as GTD expiry, trailing-stop activation, and instrument-expiration checks. A
later quote or executable bar can run that maintenance.

## Trade-driven matching

The engine temporarily moves its matching references to the trade price:

- A `SELLER` trade can match resting BUY orders.
- A `BUYER` trade can match resting SELL orders.
- A `NO_AGGRESSOR` trade can affect both sides because the passive side is unknown.

The historical order book remains unchanged. Only the matching core's transient bid, ask, and last
prices move for the iteration.

### Fill determination

When a trade triggers a limit fill:

1. If the book contains crossed liquidity, the engine fills against those book levels.
1. If the book does not represent the trade price, the engine can create a trade-driven fill at
   the order's limit price.
1. A trade-driven fill is capped at `min(order.leaves_qty, trade.size)`.

With `liquidity_consumption=False`, the same trade size can support more than one order during an
iteration. With `liquidity_consumption=True`, trade-driven fills share a consumption counter, so
their total cannot exceed the unconsumed trade size.

For example, a `SELLER` trade at 100.00 can fill a BUY LIMIT at 100.05. If no book level represents
that fill, the engine uses 100.05 rather than granting the better trade price.

### Matching-state restoration

After the iteration, the engine restores matching references from the available market baseline:

- With L2 or L3 data, the depth book remains the independent source of bid and ask state.
- With an L1 quote baseline, the non-aggressor side is restored from the latest quote.
- With trade-only L1 data, there is no quote baseline to restore, so the latest trade continues to
  define the available top-of-book state.

This distinction matters when interpreting a stream of trades without quotes. Repeated trades can
move the simulated L1 state, but quote-backed L1 matching does not progressively discard the
non-aggressor side of the latest quote.

## Aggressor sides

The aggressor is the participant that crossed the spread:

- `SELLER`: A seller hit the bid. The trade can fill a resting BUY order.
- `BUYER`: A buyer lifted the ask. The trade can fill a resting SELL order.
- `NO_AGGRESSOR`: The data does not identify the aggressor. The engine considers both sides where
  the feature requires a side.

A BUYER trade provides evidence for passive SELL orders, not BUY orders. A SELLER trade provides
evidence for passive BUY orders, not SELL orders.

## Combining book and trade data

Book updates establish the spread and visible depth. Trade ticks provide execution evidence between
those updates. This is useful when depth snapshots are throttled and a trade occurs at a price that
the latest snapshot does not contain.

Use the two feeds with care:

- A trade must have the opposite aggressor side to fill a resting order.
- A book update can cross an order independently of a trade.
- A fill at a missing trade-price level uses the trade-driven quantity cap.
- With consumption enabled, the engine accounts for trade volume already removed from an L2 or L3
  book before triggered orders consume the remaining depth.

## Queue position tracking

Set `queue_position=True` with `trade_execution=True` to track displayed quantity ahead of each
LIMIT order:

```python
venue = BacktestVenueConfig(
    name="SIM",
    oms_type=OmsType.NETTING,
    account_type=AccountType.MARGIN,
    book_type=BookType.L2_MBP,
    starting_balances=["100_000 USD"],
    trade_execution=True,
    queue_position=True,
)
```

### Queue lifecycle

1. On acceptance, a LIMIT order snapshots same-side displayed size at its price.
1. Correct-side trades at that price reduce the quantity ahead.
1. The order becomes fill-eligible when the quantity ahead reaches zero.
1. Only trade volume beyond the cleared queue is available to fill on that tick.

For example:

1. The bid at 100.00 contains 100 units.
1. A BUY LIMIT for 50 units joins with 100 units ahead.
1. A `SELLER` trade for 80 units reduces the queue ahead to 20.
1. A `SELLER` trade for 30 units clears the queue and leaves 10 units available to fill.
1. The next correct-side trade can fill the remaining order quantity.

### Book changes

For L2 books and aggregate L3 updates:

- A DELETE clears the price level and its queue.
- An UPDATE caps quantity ahead at the level's new displayed size.

For L3 MBO books:

- A per-order DELETE advances the queue by that order's remaining tracked size.
- A size decrease advances the queue by the difference.
- A size increase keeps the larger order ahead.
- A price change removes the book order from the tracked queue.

Changing a simulated order's price resets its queue position at the new level. A quantity-only
change retains the progress already made.

### L1 queue tracking

With `BookType.L1_MBP`, trade ticks reduce quantity ahead while quotes provide price-move and
displayed-size evidence:

- A move away through the order's price clears the queue.
- A move toward the order preserves the queue.
- A return to a previously visible level caps quantity ahead at the new displayed size.
- An order behind the BBO remains pending until a quote reaches its price or a trade crosses it.

### Limitations

- Queue tracking applies only to `LIMIT` orders.
- Each simulated order has an independent queue estimate.
- The initial estimate is limited to book state visible at acceptance.
- `NO_AGGRESSOR` trades reduce queues on both sides. This can clear a queue and fill an order
  earlier than reality, so it is optimistic from the strategy's execution perspective.
- Historical data cannot reveal hidden orders or every venue-specific priority rule.
