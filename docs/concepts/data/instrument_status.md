# InstrumentStatus

`InstrumentStatus` represents a change in the trading state of an instrument. It
captures venue status events such as pre-open, trading, halt, pause, close, and
short-selling restriction changes.

## Fields

| Field                       | Rust type              | Python type   | Required/default | Notes                                      |
|-----------------------------|------------------------|---------------|------------------|--------------------------------------------|
| `instrument_id`             | `InstrumentId`         | `InstrumentId` | Required        | Instrument whose status changed.           |
| `action`                    | `MarketStatusAction`   | `MarketStatusAction` | Required | Venue status action.                       |
| `ts_event`                  | `UnixNanos`            | `int`         | Required         | Event timestamp in nanoseconds.            |
| `ts_init`                   | `UnixNanos`            | `int`         | Required         | Initialization timestamp in nanoseconds.   |
| `reason`                    | `Option<Ustr>`         | `str \| None`  | `None`           | Cause of the status change when provided.  |
| `trading_event`             | `Option<Ustr>`         | `str \| None`  | `None`           | Venue event label when provided.           |
| `is_trading`                | `Option<bool>`         | `bool \| None` | `None`           | Whether trading is enabled when known.     |
| `is_quoting`                | `Option<bool>`         | `bool \| None` | `None`           | Whether quoting is enabled when known.     |
| `is_short_sell_restricted`  | `Option<bool>`         | `bool \| None` | `None`           | Short‑sell restriction state when known.   |

## Behavior

- Optional booleans allow adapters to preserve venue-provided state without guessing.
- `action` gives the normalized high-level status even when venue-specific details are
  also stored in `reason` or `trading_event`.
- Strategies can handle status updates through `on_instrument_status(...)`.

## Example

```rust tab="Rust"
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::InstrumentStatus,
    enums::MarketStatusAction,
    identifiers::InstrumentId,
};
use ustr::Ustr;

let status = InstrumentStatus::new(
    InstrumentId::from("AAPL.XNAS"),
    MarketStatusAction::Trading,
    UnixNanos::from(1_000_000_000),
    UnixNanos::from(1_000_000_100),
    Some(Ustr::from("Normal trading")),
    Some(Ustr::from("MARKET_OPEN")),
    Some(true),
    Some(true),
    Some(false),
);
```

```python tab="Python"
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import InstrumentStatus
from nautilus_trader.model.enums import MarketStatusAction

status = InstrumentStatus(
    instrument_id=InstrumentId.from_str("AAPL.XNAS"),
    action=MarketStatusAction.TRADING,
    ts_event=1_000_000_000,
    ts_init=1_000_000_100,
    reason="Normal trading",
    trading_event="MARKET_OPEN",
    is_trading=True,
    is_quoting=True,
    is_short_sell_restricted=False,
)
```

## Related guides

- [InstrumentClose](instrument_close.md) covers instrument close price events.
- [Instruments](../instruments/) covers instrument definitions.
- [Python API Reference](/docs/python-api-latest/model/data.html) lists Python members.
