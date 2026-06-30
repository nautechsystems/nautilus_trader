# OrderBookDelta

`OrderBookDelta` represents one change to an order book. It is the most granular
built-in book data type and supports the book types Nautilus uses for incremental
book updates:

- `L3_MBO`: Level 3 market-by-order (MBO) data.
- `L2_MBP`: Level 2 market-by-price (MBP) data.
- `L1_MBP`: Level 1 market-by-price (MBP) top-of-book data.

The source feed and target `BookType` determine which granularity a delta carries.

Use it when a venue or data provider publishes incremental book changes and Nautilus
should maintain the book state locally.

## Fields

| Field           | Rust type      | Python type        | Required/default | Notes                                      |
|-----------------|----------------|--------------------|------------------|--------------------------------------------|
| `instrument_id` | `InstrumentId` | `InstrumentId`     | Required         | Instrument whose book is changing.         |
| `action`        | `BookAction`   | `BookAction`       | Required         | `ADD`, `UPDATE`, `DELETE`, or `CLEAR`.     |
| `order`         | `BookOrder`    | `BookOrder`        | Required         | Price, size, side, and order ID payload.   |
| `flags`         | `u8`           | `int`              | Required         | `RecordFlag` bit field for event metadata. |
| `sequence`      | `u64`          | `int`              | Required         | Venue sequence number, or zero if absent.  |
| `ts_event`      | `UnixNanos`    | `int`              | Required         | Event timestamp in nanoseconds.            |
| `ts_init`       | `UnixNanos`    | `int`              | Required         | Initialization timestamp in nanoseconds.   |

## BookOrder fields

The `order` field contains the `BookOrder` payload for the delta.

| Field      | Rust type         | Python type | Notes                                |
|------------|-------------------|-------------|--------------------------------------|
| `side`     | `OrderSide`       | `OrderSide` | Order side.                          |
| `price`    | `Price`           | `Price`     | Order price.                         |
| `size`     | `Quantity`        | `Quantity`  | Order size.                          |
| `order_id` | `OrderId` (`u64`) | `int`       | Order ID carried by the source feed. |

The null/default order uses `NO_ORDER_SIDE`, zero price, zero size, and `order_id` zero.

## BookAction variants

| Rust variant         | Python variant | Value | Meaning                                |
|----------------------|----------------|-------|----------------------------------------|
| `BookAction::Add`    | `ADD`          | `1`   | Adds an order to the book.             |
| `BookAction::Update` | `UPDATE`       | `2`   | Updates an existing order in the book. |
| `BookAction::Delete` | `DELETE`       | `3`   | Deletes an existing order in the book. |
| `BookAction::Clear`  | `CLEAR`        | `4`   | Clears the order book state.           |

## Behavior

- `ADD` and `UPDATE` deltas require a positive order size.
- `CLEAR` deltas reset book state and use a null book order.
- `flags` carries event boundary and snapshot metadata. See
  [Delta flags and event boundaries](index.md#delta-flags-and-event-boundaries).
- Rust provides `OrderBookDelta::clear(...)`; Python users construct clear deltas
  with `BookAction.CLEAR` and a null or default book order.

## Example

```rust tab="Rust"
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{BookOrder, OrderBookDelta},
    enums::{BookAction, OrderSide, RecordFlag},
    identifiers::InstrumentId,
    types::{Price, Quantity},
};

let delta = OrderBookDelta::new(
    InstrumentId::from("ETHUSDT-PERP.BINANCE"),
    BookAction::Add,
    BookOrder::new(
        OrderSide::Buy,
        Price::from("2500.10"),
        Quantity::from("3.5"),
        12_345,
    ),
    RecordFlag::F_LAST as u8,
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
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import RecordFlag

delta = OrderBookDelta(
    instrument_id=InstrumentId.from_str("ETHUSDT-PERP.BINANCE"),
    action=BookAction.ADD,
    order=BookOrder(
        OrderSide.BUY,
        Price.from_str("2500.10"),
        Quantity.from_str("3.5"),
        12_345,
    ),
    flags=RecordFlag.F_LAST,
    sequence=42,
    ts_event=1_000_000_000,
    ts_init=1_000_000_100,
)
```

## Related guides

- [OrderBookDeltas](order_book_deltas.md) covers batching deltas.
- [Order books](../order_book.md) explains book types and local book state.
- [Python API Reference](/docs/python-api-latest/model/data.html) lists Python members.
