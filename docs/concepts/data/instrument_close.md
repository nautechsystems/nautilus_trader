# InstrumentClose

`InstrumentClose` represents a closing price event for an instrument at a venue.
It is used for end-of-session closes and contract-expiry close events.

## Fields

| Field           | Rust type             | Python type           | Required/default | Notes                                    |
|-----------------|-----------------------|-----------------------|------------------|------------------------------------------|
| `instrument_id` | `InstrumentId`        | `InstrumentId`        | Required         | Instrument being closed.                 |
| `close_price`   | `Price`               | `Price`               | Required         | Closing or settlement price.             |
| `close_type`    | `InstrumentCloseType` | `InstrumentCloseType` | Required         | `END_OF_SESSION` or `CONTRACT_EXPIRED`.  |
| `ts_event`      | `UnixNanos`           | `int`                 | Required         | Event timestamp in nanoseconds.          |
| `ts_init`       | `UnixNanos`           | `int`                 | Required         | Initialization timestamp in nanoseconds. |

## Behavior

- End-of-session closes provide session-level close prices.
- Contract-expiry closes mark expiration events for dated contracts.
- The close price is reference data; it does not imply a trade occurred at that price.

## Example

```rust tab="Rust"
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::InstrumentClose,
    enums::InstrumentCloseType,
    identifiers::InstrumentId,
    types::Price,
};

let close = InstrumentClose::new(
    InstrumentId::from("ESM4.XCME"),
    Price::from("5325.25"),
    InstrumentCloseType::EndOfSession,
    UnixNanos::from(1_000_000_000),
    UnixNanos::from(1_000_000_100),
);
```

```python tab="Python"
from nautilus_trader.model import InstrumentClose
from nautilus_trader.model import InstrumentCloseType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price

close = InstrumentClose(
    instrument_id=InstrumentId.from_str("ESM4.XCME"),
    close_price=Price.from_str("5325.25"),
    close_type=InstrumentCloseType.END_OF_SESSION,
    ts_event=1_000_000_000,
    ts_init=1_000_000_100,
)
```

## Related guides

- [InstrumentStatus](instrument_status.md) covers instrument status events.
- [Instruments](../instruments/) covers instrument definitions.
- [Python API Reference](/docs/python-api-latest/model/data.html) lists Python members.
