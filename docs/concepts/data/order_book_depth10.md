# OrderBookDepth10

`OrderBookDepth10` represents a fixed-depth book update with up to 10 bid levels
and 10 ask levels. It is useful when a venue publishes a self-contained depth
snapshot rather than incremental deltas.

## Fields

| Field           | Rust type          | Python type       | Required/default | Notes                                      |
|-----------------|--------------------|-------------------|------------------|--------------------------------------------|
| `instrument_id` | `InstrumentId`     | `InstrumentId`    | Required         | Instrument whose book is represented.      |
| `bids`          | `[BookOrder; 10]`  | `list[BookOrder]` | Required         | Exactly 10 bid levels.                     |
| `asks`          | `[BookOrder; 10]`  | `list[BookOrder]` | Required         | Exactly 10 ask levels.                     |
| `bid_counts`    | `[u32; 10]`        | `list[int]`       | Required         | Number of bid orders at each level.        |
| `ask_counts`    | `[u32; 10]`        | `list[int]`       | Required         | Number of ask orders at each level.        |
| `flags`         | `u8`               | `int`             | Required         | `RecordFlag` bit field for event metadata. |
| `sequence`      | `u64`              | `int`             | Required         | Venue sequence number, or zero if absent.  |
| `ts_event`      | `UnixNanos`        | `int`             | Required         | Event timestamp in nanoseconds.            |
| `ts_init`       | `UnixNanos`        | `int`             | Required         | Initialization timestamp in nanoseconds.   |

## Behavior

- Rust and PyO3 Python constructors require exactly 10 bid levels, 10 ask levels,
  10 bid counts, and 10 ask counts.
- Use null or default book orders with zero counts for unavailable levels.
- This type is not interchangeable with incremental `OrderBookDelta` streams.

## Example

```rust tab="Rust"
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{BookOrder, OrderBookDepth10, DEPTH10_LEN},
    enums::OrderSide,
    identifiers::InstrumentId,
    types::{Price, Quantity},
};

let mut bids = [BookOrder::default(); DEPTH10_LEN];
let mut asks = [BookOrder::default(); DEPTH10_LEN];
bids[0] = BookOrder::new(OrderSide::Buy, Price::from("2500.10"), Quantity::from("3.5"), 1);
asks[0] = BookOrder::new(OrderSide::Sell, Price::from("2500.20"), Quantity::from("2.0"), 2);

let depth = OrderBookDepth10::new(
    InstrumentId::from("ETHUSDT-PERP.BINANCE"),
    bids,
    asks,
    [1; DEPTH10_LEN],
    [1; DEPTH10_LEN],
    0,
    42,
    UnixNanos::from(1_000_000_000),
    UnixNanos::from(1_000_000_100),
);
```

```python tab="Python"
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import OrderBookDepth10
from nautilus_trader.model.enums import OrderSide

bids = [
    BookOrder(
        OrderSide.BUY,
        Price.from_str(f"{2500.10 - i * 0.10:.2f}"),
        Quantity.from_str("3.5"),
        i + 1,
    )
    for i in range(10)
]
asks = [
    BookOrder(
        OrderSide.SELL,
        Price.from_str(f"{2500.20 + i * 0.10:.2f}"),
        Quantity.from_str("2.0"),
        i + 11,
    )
    for i in range(10)
]

depth = OrderBookDepth10(
    instrument_id=InstrumentId.from_str("ETHUSDT-PERP.BINANCE"),
    bids=bids,
    asks=asks,
    bid_counts=[1] * 10,
    ask_counts=[1] * 10,
    flags=0,
    sequence=42,
    ts_event=1_000_000_000,
    ts_init=1_000_000_100,
)
```

## Related guides

- [QuoteTick](quote_tick.md) covers top-of-book data derived from depth.
- [Order books](index.md#order-books) explains order book state.
- [Python API Reference](/docs/python-api-latest/model/data.html) lists Python members.
