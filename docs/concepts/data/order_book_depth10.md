# OrderBookDepth

`OrderBookDepth` represents a snapshot with a variable number of bid and ask levels.
Use it when a venue publishes a self-contained depth snapshot rather than incremental deltas.

## Fields

| Field           | Rust type                   | Python type       | Required/default | Notes                                      |
| --------------- | --------------------------- | ----------------- | ---------------- | ------------------------------------------ |
| `instrument_id` | `InstrumentId`              | `InstrumentId`    | Required         | Instrument whose book is represented.      |
| `bids`          | `SmallVec<[BookOrder; 10]>` | `list[BookOrder]` | Required         | Bid levels in book order.                  |
| `asks`          | `SmallVec<[BookOrder; 10]>` | `list[BookOrder]` | Required         | Ask levels in book order.                  |
| `bid_counts`    | `SmallVec<[u32; 10]>`       | `list[int]`       | Required         | Number of bid orders at each level.        |
| `ask_counts`    | `SmallVec<[u32; 10]>`       | `list[int]`       | Required         | Number of ask orders at each level.        |
| `flags`         | `u8`                        | `int`             | Required         | `RecordFlag` bit field for event metadata. |
| `sequence`      | `u64`                       | `int`             | Required         | Venue sequence number, or zero if absent.  |
| `ts_event`      | `UnixNanos`                 | `int`             | Required         | Event timestamp in nanoseconds.            |
| `ts_init`       | `UnixNanos`                 | `int`             | Required         | Initialization timestamp in nanoseconds.   |

## Behavior

- Rust and PyO3 Python constructors accept variable-length sides. Each side requires one count
  per order; bid and ask sides can have different lengths.
- Empty sides use empty sequences. The inline capacity is ten; larger snapshots allocate as needed.
- This type is not interchangeable with incremental `OrderBookDelta` streams.

`OrderBookDepth10` remains a compatibility name. The legacy C FFI and fixed-depth SBE encoding require exactly ten
levels per side.

## Example

```rust tab="Rust"
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{BookOrder, OrderBookDepth},
    enums::OrderSide,
    identifiers::InstrumentId,
    types::{Price, Quantity},
};

let bids = vec![BookOrder::new(OrderSide::Buy, Price::from("2500.10"), Quantity::from("3.5"), 1)];
let asks = vec![BookOrder::new(OrderSide::Sell, Price::from("2500.20"), Quantity::from("2.0"), 2)];

let depth = OrderBookDepth::new(
    InstrumentId::from("ETHUSDT-PERP.BINANCE"),
    bids,
    asks,
    vec![1],
    vec![1],
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
from nautilus_trader.model import BookOrder
from nautilus_trader.model import OrderBookDepth
from nautilus_trader.model import OrderSide

bids = [
    BookOrder(
        OrderSide.BUY,
        Price.from_str(f"{2500.10 - i * 0.10:.2f}"),
        Quantity.from_str("3.5"),
        i + 1,
    )
    for i in range(3)
]
asks = [
    BookOrder(
        OrderSide.SELL,
        Price.from_str(f"{2500.20 + i * 0.10:.2f}"),
        Quantity.from_str("2.0"),
        i + 11,
    )
    for i in range(2)
]

depth = OrderBookDepth(
    instrument_id=InstrumentId.from_str("ETHUSDT-PERP.BINANCE"),
    bids=bids,
    asks=asks,
    bid_counts=[1] * len(bids),
    ask_counts=[1] * len(asks),
    flags=0,
    sequence=42,
    ts_event=1_000_000_000,
    ts_init=1_000_000_100,
)
```

## Related guides

- [QuoteTick](quote_tick.md) covers top-of-book data derived from depth.
- [Order books](index.md#order-books) explains order book state.
- [Python API reference](/docs/python-api-latest/model/data.html) lists Python members.
