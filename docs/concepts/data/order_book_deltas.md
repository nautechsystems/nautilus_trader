# OrderBookDeltas

`OrderBookDeltas` groups a non-empty batch of `OrderBookDelta` records that belong
to the same logical book event. It reduces per-message overhead when adapters receive
or produce multiple book changes at once.

## Fields

| Field           | Rust type             | Python type            | Required/default | Notes                                     |
|-----------------|-----------------------|------------------------|------------------|-------------------------------------------|
| `instrument_id` | `InstrumentId`        | `InstrumentId`         | Required         | Instrument whose book is changing.        |
| `deltas`        | `Vec<OrderBookDelta>` | `list[OrderBookDelta]` | Required         | Non‑empty batch of deltas.                |
| `flags`         | `u8`                  | `int`                  | From last delta  | Last delta flags.                         |
| `sequence`      | `u64`                 | `int`                  | From last delta  | Last delta sequence number.               |
| `ts_event`      | `UnixNanos`           | `int`                  | From last delta  | Last delta event timestamp.               |
| `ts_init`       | `UnixNanos`           | `int`                  | From last delta  | Last delta initialization timestamp.      |

## Behavior

- The batch must contain at least one delta.
- The batch metadata mirrors the final delta.
- The final delta should carry `F_LAST` when it closes a logical event group.
- Snapshot batches usually begin with a `CLEAR` delta and end with `F_SNAPSHOT | F_LAST`.

## Example

```rust tab="Rust"
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas},
    enums::{BookAction, OrderSide, RecordFlag},
    identifiers::InstrumentId,
    types::{Price, Quantity},
};

let instrument_id = InstrumentId::from("ETHUSDT-PERP.BINANCE");
let bid = OrderBookDelta::new(
    instrument_id,
    BookAction::Add,
    BookOrder::new(OrderSide::Buy, Price::from("2500.10"), Quantity::from("3.5"), 1),
    0,
    41,
    UnixNanos::from(1_000_000_000),
    UnixNanos::from(1_000_000_100),
);
let ask = OrderBookDelta::new(
    instrument_id,
    BookAction::Add,
    BookOrder::new(OrderSide::Sell, Price::from("2500.20"), Quantity::from("2.0"), 2),
    RecordFlag::F_LAST as u8,
    42,
    UnixNanos::from(1_000_000_000),
    UnixNanos::from(1_000_000_100),
);

let deltas = OrderBookDeltas::new(instrument_id, vec![bid, ask]);
```

```python tab="Python"
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import RecordFlag

instrument_id = InstrumentId.from_str("ETHUSDT-PERP.BINANCE")
bid = OrderBookDelta(
    instrument_id=instrument_id,
    action=BookAction.ADD,
    order=BookOrder(
        OrderSide.BUY,
        Price.from_str("2500.10"),
        Quantity.from_str("3.5"),
        1,
    ),
    flags=0,
    sequence=41,
    ts_event=1_000_000_000,
    ts_init=1_000_000_100,
)
ask = OrderBookDelta(
    instrument_id=instrument_id,
    action=BookAction.ADD,
    order=BookOrder(
        OrderSide.SELL,
        Price.from_str("2500.20"),
        Quantity.from_str("2.0"),
        2,
    ),
    flags=RecordFlag.F_LAST,
    sequence=42,
    ts_event=1_000_000_000,
    ts_init=1_000_000_100,
)

deltas = OrderBookDeltas(instrument_id, [bid, ask])
```

## Related guides

- [OrderBookDelta](order_book_delta.md) covers the contained update type.
- [Order books](../order_book.md) explains supported order book state.
- [Python API Reference](/docs/python-api-latest/model/data.html) lists Python members.
