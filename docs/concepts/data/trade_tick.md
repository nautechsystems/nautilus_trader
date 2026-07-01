# TradeTick

`TradeTick` represents one executed trade or match event from a venue. It carries
the traded price, traded size, aggressor side, and venue trade identifier.

## Fields

| Field            | Rust type       | Python type     | Required/default | Notes                                    |
|------------------|-----------------|-----------------|------------------|------------------------------------------|
| `instrument_id`  | `InstrumentId`  | `InstrumentId`  | Required         | Instrument for the trade.                |
| `price`          | `Price`         | `Price`         | Required         | Executed price.                          |
| `size`           | `Quantity`      | `Quantity`      | Required         | Executed quantity.                       |
| `aggressor_side` | `AggressorSide` | `AggressorSide` | Required         | Buyer, seller, or no aggressor.          |
| `trade_id`       | `TradeId`       | `TradeId`       | Required         | Venue‑assigned match ID.                 |
| `ts_event`       | `UnixNanos`     | `int`           | Required         | Event timestamp in nanoseconds.          |
| `ts_init`        | `UnixNanos`     | `int`           | Required         | Initialization timestamp in nanoseconds. |

## Behavior

- `size` must be positive.
- Information-driven bars require `TradeTick` data because they use `aggressor_side`.
- Trade bars use `LAST` price type.
- `trade_id` should be stable for the venue event when the venue provides one.

## Example

```rust tab="Rust"
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::TradeTick,
    enums::AggressorSide,
    identifiers::{InstrumentId, TradeId},
    types::{Price, Quantity},
};

let trade = TradeTick::new(
    InstrumentId::from("BTCUSDT.BINANCE"),
    Price::from("65000.10"),
    Quantity::from("0.25"),
    AggressorSide::Buyer,
    TradeId::from("123456789"),
    UnixNanos::from(1_000_000_000),
    UnixNanos::from(1_000_000_100),
);
```

```python tab="Python"
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import TradeId
from nautilus_trader.model import TradeTick
from nautilus_trader.model.enums import AggressorSide

trade = TradeTick(
    instrument_id=InstrumentId.from_str("BTCUSDT.BINANCE"),
    price=Price.from_str("65000.10"),
    size=Quantity.from_str("0.25"),
    aggressor_side=AggressorSide.BUYER,
    trade_id=TradeId("123456789"),
    ts_event=1_000_000_000,
    ts_init=1_000_000_100,
)
```

## Related guides

- [Bar](bar.md) covers trade-to-bar aggregation.
- [Information-driven bars](index.md#information-driven-bars) explains aggressor-side use.
- [Python API Reference](/docs/python-api-latest/model/data.html) lists Python members.
