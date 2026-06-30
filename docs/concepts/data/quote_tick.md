# QuoteTick

`QuoteTick` represents the top-of-book bid and ask for one instrument. It carries
the best available bid price and size, and the best available ask price and size,
at a specific event time.

## Fields

| Field           | Rust type      | Python type    | Required/default | Notes                                    |
|-----------------|----------------|----------------|------------------|------------------------------------------|
| `instrument_id` | `InstrumentId` | `InstrumentId` | Required         | Instrument for the quote.                |
| `bid_price`     | `Price`        | `Price`        | Required         | Best bid price.                          |
| `ask_price`     | `Price`        | `Price`        | Required         | Best ask price.                          |
| `bid_size`      | `Quantity`     | `Quantity`     | Required         | Quantity available at the best bid.      |
| `ask_size`      | `Quantity`     | `Quantity`     | Required         | Quantity available at the best ask.      |
| `ts_event`      | `UnixNanos`    | `int`          | Required         | Event timestamp in nanoseconds.          |
| `ts_init`       | `UnixNanos`    | `int`          | Required         | Initialization timestamp in nanoseconds. |

## Behavior

- Bid and ask prices must use the same precision.
- Bid and ask sizes must use the same precision.
- `extract_price(PriceType.BID | ASK | MID)` returns the requested price basis.
- Quote bars can use `BID`, `ASK`, or `MID` price types.

## Example

```rust tab="Rust"
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::QuoteTick,
    identifiers::InstrumentId,
    types::{Price, Quantity},
};

let quote = QuoteTick::new(
    InstrumentId::from("AUD/USD.SIM"),
    Price::from("0.65000"),
    Price::from("0.65002"),
    Quantity::from("1000000"),
    Quantity::from("1200000"),
    UnixNanos::from(1_000_000_000),
    UnixNanos::from(1_000_000_100),
);
```

```python tab="Python"
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick

quote = QuoteTick(
    instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
    bid_price=Price.from_str("0.65000"),
    ask_price=Price.from_str("0.65002"),
    bid_size=Quantity.from_int(1_000_000),
    ask_size=Quantity.from_int(1_200_000),
    ts_event=1_000_000_000,
    ts_init=1_000_000_100,
)
```

## Related guides

- [OrderBookDepth10](order_book_depth10.md) covers fixed-depth snapshots with top levels.
- [Bars and aggregation](index.md#bars-and-aggregation) covers quote-to-bar aggregation.
- [Python API Reference](/docs/python-api-latest/model/data.html) lists Python members.
