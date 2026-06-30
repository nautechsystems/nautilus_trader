# FundingRateUpdate

`FundingRateUpdate` represents the current funding rate for a perpetual swap
instrument. It can also include the funding interval and the next funding timestamp
when the venue publishes them.

## Fields

| Field             | Rust type             | Python type      | Required/default | Notes                                    |
|-------------------|-----------------------|------------------|------------------|------------------------------------------|
| `instrument_id`   | `InstrumentId`        | `InstrumentId`   | Required         | Perpetual instrument for the rate.       |
| `rate`            | `Decimal`             | `Decimal`        | Required         | Current funding rate.                    |
| `interval`        | `Option<u16>`         | `int \| None`    | `None`           | Funding interval in minutes.             |
| `next_funding_ns` | `Option<UnixNanos>`   | `int \| None`    | `None`           | Next funding timestamp in nanoseconds.   |
| `ts_event`        | `UnixNanos`           | `int`            | Required         | Event timestamp in nanoseconds.          |
| `ts_init`         | `UnixNanos`           | `int`            | Required         | Initialization timestamp in nanoseconds. |

## Behavior

- Equality and hashing use instrument ID, rate, interval, and next funding time.
- Funding rates are reference data and do not imply a payment was applied.
- Use `interval` and `next_funding_ns` only when the venue publishes them.

## Example

```rust tab="Rust"
use nautilus_core::UnixNanos;
use nautilus_model::{data::FundingRateUpdate, identifiers::InstrumentId};
use rust_decimal::Decimal;

let funding = FundingRateUpdate::new(
    InstrumentId::from("BTCUSDT-PERP.BINANCE"),
    Decimal::new(1, 4),
    Some(480),
    Some(UnixNanos::from(1_000_008_000)),
    UnixNanos::from(1_000_000_000),
    UnixNanos::from(1_000_000_100),
);
```

```python tab="Python"
from decimal import Decimal

from nautilus_trader.model import FundingRateUpdate
from nautilus_trader.model import InstrumentId

funding = FundingRateUpdate(
    instrument_id=InstrumentId.from_str("BTCUSDT-PERP.BINANCE"),
    rate=Decimal("0.0001"),
    ts_event=1_000_000_000,
    ts_init=1_000_000_100,
    interval=480,
    next_funding_ns=1_000_008_000,
)
```

## Related guides

- [MarkPriceUpdate](mark_price_update.md) covers mark prices for derivatives.
- [IndexPriceUpdate](index_price_update.md) covers index reference prices.
- [Python API Reference](/docs/python-api-latest/model/data.html) lists Python members.
