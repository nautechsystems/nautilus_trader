# Trade-based execution

Trade tick data triggers order fills by default (`trade_execution=True`). A trade tick indicates
that liquidity was accessed at the trade price, allowing resting limit orders to match. This mirrors
the default behavior for bar data (`bar_execution=True`).

Advanced users who want to isolate execution to L1 book data only (quotes or order book updates) can
disable trade-based execution:

```python
venue_config = BacktestVenueConfig(
    name="SIM",
    oms_type="NETTING",
    account_type="CASH",
    starting_balances=["100_000 USD"],
    trade_execution=False,  # Disable trade-based fills
)
```

When `trade_execution=False` or `bar_execution=False`, the respective data types skip order matching
and maintenance operations (GTD order expiry, trailing stop activation, instrument expiration
checks). Quote ticks always trigger maintenance, so this is typically acceptable when using multiple
data types.

The matching engine uses a "transient override" mechanism: during the matching process, it
temporarily adjusts the matching core's Best Bid (for BUYER trades) or Best Ask (for SELLER trades)
toward the trade price. This allows resting orders on the passive side to cross the spread and fill.
Note: the underlying order book data is never modified (it remains immutable); only the matching
core's internal price references are adjusted.

**Fill determination:**

When a trade tick triggers order matching, the engine determines fills as follows:

1. **Book reflects trade price**: If the order book has liquidity at the trade price, fills use book
   depth (standard behavior).
2. **Book doesn't reflect trade price**: If the book's liquidity is at a different price, the engine
   uses a "trade-driven fill" at the order's limit price, capped to `min(order.leaves_qty,
   trade.size)`.

This ensures that when a trade prints through the spread but the book hasn't updated, fills are
bounded by what the trade tick actually evidences. When `liquidity_consumption=False` (default), the
same trade size can fill multiple orders within an iteration. When `liquidity_consumption=True`,
consumption tracking applies to trade-driven fills as well. Repeated fills at the same trade price
will be bounded by consumed liquidity until fresh data arrives.

**Restoration behavior:**

After matching, the core's bid/ask are only restored to their original values if the trade price
improved them (moved them away from the spread):

- **SELLER trade**: Ask is restored only if trade price was below the original ask.
- **BUYER trade**: Bid is restored only if trade price was above the original bid.

If the trade price didn't improve the quote (e.g., a SELLER trade at or above the ask), the core
retains the trade price. This means repeated trades at or beyond the spread can progressively move
the core's bid/ask.

**Fill price:**

- **SELLER trade at P**: The engine sets the core's Best Ask to P (if P < current ask). Resting BUY
  LIMIT orders at P or higher will fill at their limit price (if book doesn't have that level) or at
  book prices (if book does).
- **BUYER trade at P**: The engine sets the core's Best Bid to P (if P > current bid). Resting SELL
  LIMIT orders at P or lower will fill at their limit price (if book doesn't have that level) or at
  book prices (if book does).

This conservative approach ensures fills occur at the order's limit price rather than potentially
better trade prices. For example, a BUY LIMIT at 100.05 triggered by a SELLER trade at 100.00 will
fill at 100.05, not 100.00.

:::tip
Combine trade data with book or quote data for best results: book/quote data establishes the
baseline spread, while trade ticks trigger execution for orders that might be inside the spread or
ahead of the quote updates.
:::

## Understanding trade tick aggressor sides

A common source of confusion is the `aggressor_side` field on trade ticks:

- **SELLER trade**: A seller aggressed, selling into the bid. This provides evidence of fill-able
  liquidity for **BUY** orders at the trade price.
- **BUYER trade**: A buyer aggressed, buying from the ask. This provides evidence of fill-able
  liquidity for **SELL** orders at the trade price.

In other words, trade ticks trigger fills for orders on the **opposite** side of the aggressor. A
SELLER trade at 100.00 can fill your resting BUY LIMIT at 100.00, but cannot fill your SELL LIMIT,
since the trade already represents someone else selling.

## Combining L2 book data with trade ticks

When using L2 order book data (e.g., 100ms throttled depth snapshots) combined with trade tick data:

1. **Book updates establish the spread**: Each book delta/snapshot updates the matching engine's
   view of available liquidity at each price level.

2. **Trade ticks provide execution evidence**: Trade ticks indicate that liquidity was accessed at a
   specific price, potentially between book snapshots.

3. **Fill quantity determination**: When a trade triggers a fill:
   - If the book already reflects liquidity at the trade price, fills use book depth
   - If the trade price is inside the spread (not in the current book), fills are capped by `min(order.leaves_qty, trade.size)`

4. **Timing considerations**: With throttled book data (e.g., 100ms), the book may lag behind
   trades. A trade at a price not yet reflected in the book will use trade-driven fill logic.

**Common misconception**: Users sometimes expect every trade tick to trigger fills. Remember:

- Only trades on the **opposite** side can fill your orders.
- SELLER trades -> potential BUY fills.
- BUYER trades -> potential SELL fills.
- Book UPDATE events move the market but only trigger fills if prices cross your order.

## Queue position tracking

When `queue_position=True` is enabled alongside `trade_execution=True`, the matching engine
simulates queue position for limit orders. This provides more realistic fill behavior by tracking
how many orders are "ahead" of your order at a given price level.

**How it works:**

1. **Order placement**: When a LIMIT order is accepted, the engine snapshots the current same-side
   book depth at the order's price level. This represents the orders ahead in the queue.

2. **Trade ticks**: When trade ticks occur at the order's price level, the "quantity ahead" is
   decremented by the trade size. Only trades on the correct side affect the queue (BUYER trades
   decrement queue for SELL orders, SELLER trades decrement queue for BUY orders). Trades with
   `NO_AGGRESSOR` (common in historical datasets lacking aggressor metadata) affect both sides.
   This is pessimistic but prevents orders from stalling indefinitely.

3. **Fill eligibility**: The order becomes eligible to fill only when the quantity ahead reaches
   zero.
   On the tick that clears the queue, only the excess volume (trade size minus queue ahead) is
   available for fill, preventing overfill.

4. **Book deletions**:
   - L2 (and aggregate `F_MBP`/`F_TOB` deletes in L3 books): the DELETE removes the whole price
     level, so the queue clears and the order becomes fill-eligible.
   - L3 (MBO): a per-order DELETE advances the queue by that order's remaining tracked size.
     Orders already consumed by trades or aggregate updates are not counted twice.

5. **Book updates**:
   - L2 (and aggregate `F_MBP`/`F_TOB` updates in L3 books): the UPDATE caps the quantity ahead
     at the level's new displayed size.
   - L3 (MBO): a per-order UPDATE tracks the order's size change. A decrease advances the queue
     by the difference (time priority retained). An increase keeps the order ahead with its
     larger size. A price change removes it from the queue.

6. **Order modification**: A price change resets the queue position and the order joins the back
   of the queue at its new price level. Quantity-only changes keep the accrued position.

**Configuration:**

```python
from nautilus_trader.backtest.config import BacktestVenueConfig

venue_config = BacktestVenueConfig(
    name="SIM",
    oms_type="NETTING",
    account_type="MARGIN",
    starting_balances=["100_000 USD"],
    trade_execution=True,  # Required for queue_position
    queue_position=True,  # Enable queue position tracking
)
```

**Example scenario:**

1. Order book shows 100 units at bid 100.00.
2. You place a BUY LIMIT at 100.00 for 50 units. Queue ahead = 100.
3. SELLER trade of 80 units at 100.00 -> queue ahead = 20. No fill yet.
4. SELLER trade of 30 units at 100.00 -> queue clears with 10 excess. Fill = 10 units.
5. Next SELLER trade of 50 units -> fill remaining 40 units.

**Limitations:**

- Only applies to `LIMIT` orders. Stop-limit and limit-if-touched orders are not tracked in this
  implementation.
- Queue position is per-order, not shared across multiple orders at the same price.
- The queue snapshot is based on book state at order acceptance time.
- Trades with `NO_AGGRESSOR` decrement queue for both sides, which may cause orders to fill sooner
  than in reality (pessimistic for queue estimation, but prevents stalling).

**L1 quote-based mode:**

When using `BookType.L1_MBP` (top-of-book quotes only), queue position tracking uses trade ticks to
decrement the queue (the same mechanism as L2/L3), while quote ticks handle price-move detection and
deferred snapshot resolution.

- **Trade ticks**: Trades at the order's price level decrement the queue ahead by the trade
  size, identical to L2/L3 behavior. Only trades on the correct aggressor side affect the
  queue (SELLER trades decrement queue for BUY orders, BUYER trades for SELL orders).
- **Price moves away**: If the bid drops below a BUY order's price (or ask rises above a
  SELL order's price), the order's price level has been "crossed" and the queue clears to zero,
  making the order fill-eligible on the next matching trade.
- **Price moves toward**: If the bid rises (or ask drops), the level at the order's price was
  not consumed, so queue positions are preserved.
- **Price returns to a level**: When the price returns after moving away, the queue ahead is
  capped at the new displayed size if it was previously larger.
- **Orders behind BBO (pending)**: When a limit order is placed behind the best bid/ask
  (e.g., BUY below best bid), the queue snapshot is deferred because L1 data has no visible
  depth at that level. Fills are blocked until the BBO reaches the order's price, at which
  point the queue is snapshotted from the displayed size. Pending orders are also resolved
  when trades cross through their price level.

L1 mode uses the same configuration: set `queue_position=True` with `book_type=BookType.L1_MBP`.
This provides a lightweight alternative to full L2/L3 data when only top-of-book quotes are
available.

:::note
Queue position tracking provides a heuristic simulation of queue dynamics. Real exchange queue
behavior depends on many factors (order priority rules, hidden orders, etc.) that cannot be
perfectly reconstructed from historical data.
:::
