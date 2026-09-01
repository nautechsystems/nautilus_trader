# Order Book

NautilusTrader implements its order books in Rust. `OrderBook` maintains public market depth for an instrument.
`OwnOrderBook` tracks your own orders separately so filtered views can subtract them
from public liquidity.

:::note
This guide uses the Rust model API for book operations. Subscription and handler examples use the
Python strategy and actor API. Python exposes the book types as
`nautilus_trader.model.OrderBook` and `nautilus_trader.model.OwnOrderBook`; see the
[model API reference](/docs/python-api-latest/model/book.html) for the Python interface.
:::

## Book types

`OrderBook` instances are maintained per instrument for both backtesting and live trading:

- `L3_MBO`: Level 3 market-by-order (MBO) data. Tracks every order at every price
  level, keyed by order ID. On each book side, an order ID maps to exactly one price
  level: re-adding an ID at a different price moves the order to the new level. MBP-style
  input uses a price-derived ID. A zero order ID likewise signals missing identity, except
  that top-of-book input uses the order side as its ID.
- `L2_MBP`: Level 2 market-by-price (MBP) data. Aggregates orders by price level
  (one entry per price).
- `L1_MBP`: Level 1 market-by-price (MBP) top-of-book data, also known as best bid
  and offer (BBO). Captures only the best prices.

:::note
Quote, trade, and bar data (`QuoteTick`, `TradeTick`, and `Bar`) can also drive
`L1_MBP` books.
:::

## Subscribing to book data

Strategies and actors subscribe to order book updates through the following methods.
Subscriptions and handlers are part of the Python strategy/actor layer:

```python
from nautilus_trader.model import BookType
from nautilus_trader.model import OrderBook
from nautilus_trader.model import OrderBookDeltas
from nautilus_trader.model import OrderBookDepth10


# Incremental book deltas
self.subscribe_book_deltas(instrument_id, BookType.L2_MBP)

# Aggregated depth snapshots (up to 10 levels)
self.subscribe_book_depth10(instrument_id, BookType.L2_MBP)

# Full book snapshots at a timed interval
self.subscribe_book_at_interval(instrument_id, BookType.L2_MBP, interval_ms=1000)
```

Each subscription type delivers data to the corresponding handler:

```python
def on_book_deltas(self, deltas: OrderBookDeltas) -> None: ...


def on_book_depth(self, depth: OrderBookDepth10) -> None: ...


def on_book(self, order_book: OrderBook) -> None: ...
```

## Accessing the book

The `OrderBook` exposes top-of-book accessors:

```rust
let best_bid: Option<Price> = book.best_bid_price();
let best_ask: Option<Price> = book.best_ask_price();
let spread: Option<f64> = book.spread();
let midpoint: Option<f64> = book.midpoint();
```

## Analysis methods

The `OrderBook` supports market depth analysis and execution simulation:

```rust
// Average fill price for a given quantity
let avg_fill_px = book.get_avg_px_for_quantity(quantity, OrderSide::Buy);

// Average price, filled quantity, and worst price for a target exposure
let (avg_px, filled_qty, worst_px) =
    book.get_avg_px_qty_for_exposure(target_exposure, OrderSide::Buy);

// Cumulative quantity available at or better than a price
let qty = book.get_quantity_for_price(price, OrderSide::Buy);

// Quantity at a specific price level only
let qty = book.get_quantity_at_level(price, OrderSide::Buy, 2);

// Simulate fills against the book
let fills: Vec<(Price, Quantity)> = book.simulate_fills(&order);

// All crossed levels regardless of order quantity
let levels = book.get_all_crossed_levels(OrderSide::Buy, price, 2);
```

## Integrity checks

Call `book_check_integrity` to validate that the book state is consistent with its type:

- **L1_MBP**: No more than one level per side.
- **L2_MBP**: No more than one order per price level.
- **L3_MBO**: No additional per-level constraint; multiple orders may share a price.
- **All types**: Best bid must not exceed best ask (crossed book). Locked markets
  (bid == ask) are considered valid.

This is an explicit check: applying a delta does not call it. The Rust `apply_delta` and
`apply_deltas` methods separately validate the incoming instrument ID against the book and return
`BookIntegrityError::InstrumentMismatch` on mismatch.

For a nonzero order ID, a delta whose side is `None` first tries to resolve the side from the ladder
cache. If no side is cached, an `Add` returns `BookIntegrityError::NoOrderSide`, while an `Update`
or `Delete` is skipped. If the ID exists on both sides, an `Add` returns
`BookIntegrityError::AmbiguousOrderSide`, while an `Update` or `Delete` is skipped with a warning.

Out-of-order deltas and depth snapshots are applied rather than rejected, so a venue that replays
or reorders events still reaches the state those events describe. Only the book metadata is
protected: `sequence` and `ts_last` are high-water marks and never regress. A stale update logs one
warning for each field that regressed, `sequence` and `ts_event` independently, and how often it
logs depends on how the update arrives:

- **Incremental deltas**: Once per stale delta.
- **Snapshot deltas**: Once per snapshot, whether it arrives as an `F_SNAPSHOT` batch or as a
  single `F_SNAPSHOT` delta, since every delta in a rebuild shares the snapshot's sequence and
  timestamp.
- **Depth snapshots**: Once, since an `OrderBookDepth10` replaces the book in a single update.

A snapshot report describes the incoming snapshot, so it does not depend on whether each of its
deltas reaches the book. An `L1_MBP` book driven by quotes or trades is the exception to all of
this: a stale `QuoteTick` or `TradeTick` is skipped with a warning and leaves the book unchanged.

## Pretty printing

Both `OrderBook` and `OwnOrderBook` provide a `pprint` method that returns the book as a
human-readable table:

```rust
println!("{}", book.pprint(5, None));
println!("{}", book.pprint(5, Some(Decimal::new(1, 2)))); // group_size = 0.01
```

The `group_size` parameter buckets price levels into coarser groups for instruments
with fine tick sizes. The output is a formatted table with bids on the left, prices
in the center, and asks on the right.

## Own order book

The `OwnOrderBook` tracks your own working orders separately from the public book. Market
making and other quoting strategies use it to estimate available liquidity at each price
level after subtracting their own orders.

Execution engines maintain own books when `manage_own_order_books` is enabled. The cache
updates an existing own book as order events change state. Eligible orders have a price and
do not use `IOC` or `FOK` time in force. Terminal events may still clean up an existing own
book entry, even when the order would not otherwise be eligible for tracking.

### Order lifecycle

The `OwnOrderBook` tracks orders through their lifecycle. Orders are added during submission or
materialized from reconciliation. Nonterminal states such as `OrderStatus::Accepted`,
`OrderStatus::PendingUpdate`, `OrderStatus::PendingCancel`, and `OrderStatus::PartiallyFilled`
update the entry. The closed states `OrderStatus::Denied`, `OrderStatus::Rejected`,
`OrderStatus::Canceled`, `OrderStatus::Expired`, `OrderStatus::Filled`, and `OrderStatus::Voided`
remove it.

Each `OwnBookOrder` carries:

- `trader_id`: Trader ID that owns the order.
- `client_order_id`: Client order ID used to reconcile the own book with cache state.
- `venue_order_id`: Venue order ID when one has been assigned.
- `side`, `price`, and `size`: Order side, price, and remaining (leaves) quantity.
- `order_type` and `time_in_force`: Order metadata retained for inspection.
- `status`: Current order status, such as `SUBMITTED`, `ACCEPTED`, or `PENDING_CANCEL`.
- `ts_last`: Timestamp of the latest order event applied to this own-book order.
- `ts_accepted`: Timestamp when the venue accepted the order, or zero before acceptance.
- `ts_submitted`: Timestamp when the order was submitted, or zero before submission.
- `ts_init`: Timestamp when the order was initialized.

The `status` and `ts_accepted` fields drive the optional filters described in
[Status and time filtering](#status-and-time-filtering).

### Auditing

The `audit_open_orders` method reconciles an own book against a set of valid client order
IDs. Any own-book order not in the provided set is removed and logged as an audit error.
`Cache::audit_own_order_books` builds this set from open, in-flight, and active-local orders so
non-terminal entries remain during normal event-processing and venue-latency windows. Live systems
can run this audit periodically through the own-books audit interval.

### Querying

```rust
// Check if a specific order is tracked
let in_book = own_book.is_order_in_book(&client_order_id);

// Get all tracked order IDs per side
let bid_ids = own_book.bid_client_order_ids();
let ask_ids = own_book.ask_client_order_ids();

// Aggregated quantities per price level
let bid_qty = own_book.bid_quantity(None, None, None, None, None);
let ask_qty = own_book.ask_quantity(None, None, None, None, None);

// Pretty print
println!("{}", own_book.pprint(5, None));
```

### Filtered views

Subtract your own orders from the public book to see net available liquidity:

```rust
// Filtered maps of price -> quantity (own orders subtracted)
let net_bids = book.bids_filtered_as_map(Some(10), Some(&own_book), None, None, None);
let net_asks = book.asks_filtered_as_map(Some(10), Some(&own_book), None, None, None);

// Full filtered OrderBook with all analysis methods available
let filtered = book.filtered_view(Some(&own_book), Some(10), None, None, None);
let avg_px = filtered.get_avg_px_for_quantity(quantity, OrderSide::Buy);
```

The `filtered_view` method returns a new `OrderBook` with your own sizes subtracted,
giving access to the full set of analysis methods (`spread`, `midpoint`,
`get_avg_px_for_quantity`, etc.) on the net book.

### Status and time filtering

Filtered views support optional status and time-based filtering for own orders:

```rust
let statuses = AHashSet::from([OrderStatus::Accepted]);

// Only subtract ACCEPTED orders (ignore SUBMITTED, PENDING_CANCEL, etc.)
let filtered = book.filtered_view(Some(&own_book), None, Some(&statuses), None, None);
```

The `accepted_buffer_ns` parameter provides a grace period. When `ts_now` is set, the view includes
an own order only when `ts_accepted + accepted_buffer_ns <= ts_now`. This excludes recently accepted
orders that may not yet appear in the public book feed. The time check applies regardless of order
status, so combine it with a status filter to exclude non-accepted orders. Omitting `ts_now`
disables acceptance-time filtering, and a positive `accepted_buffer_ns` requires `ts_now`.

```rust
// Only subtract orders accepted at least 500ms ago
let filtered = book.filtered_view(
    Some(&own_book),
    None,
    None,
    Some(500_000_000),
    Some(clock.timestamp_ns().as_u64()),
);
```

## Binary markets

Binary markets can expose complementary outcome instruments, such as Polymarket YES and NO tokens.
For a known complementary pair, the parity transform maps a price `p` on one outcome to `1 - p` on
the other. Under this transform, a NO bid at 0.40 becomes a YES ask at 0.60.

The `OwnOrderBook::combined_with_opposite` method handles this transformation,
merging orders from both outcome instruments into a view for the first book:

```rust
let yes_own = own_yes_book
    .cloned()
    .unwrap_or_else(|| OwnOrderBook::new(yes_instrument_id));

let no_own = own_no_book
    .cloned()
    .unwrap_or_else(|| OwnOrderBook::new(no_instrument_id));

// Merge NO orders with the parity price transform (1 - price)
let combined = yes_own.combined_with_opposite(&no_own).unwrap();

// Filter the public YES book using the combined own book
let filtered = book.filtered_view(Some(&combined), None, None, None, None);
```

The transformation works as follows:

- NO asks at price `p` become bids at price `1 - p` in the combined book.
- NO bids at price `p` become asks at price `1 - p` in the combined book.

The method rejects matching instrument IDs, but it cannot verify that the two instruments are
complementary. The caller must supply the actual opposite instrument. The resulting own book can
filter the public YES book against your orders in either outcome instrument.
