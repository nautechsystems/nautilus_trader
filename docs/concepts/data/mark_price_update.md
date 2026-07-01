# MarkPriceUpdate

`MarkPriceUpdate` represents the current mark price for an instrument. Venues use
mark prices most often for derivatives margining, liquidation checks, and unrealized
PnL calculations.

## Fields

| Field           | Rust type      | Python type    | Required/default | Notes                                    |
|-----------------|----------------|----------------|------------------|------------------------------------------|
| `instrument_id` | `InstrumentId` | `InstrumentId` | Required         | Instrument for the mark price.           |
| `value`         | `Price`        | `Price`        | Required         | Current mark price.                      |
| `ts_event`      | `UnixNanos`    | `int`          | Required         | Event timestamp in nanoseconds.          |
| `ts_init`       | `UnixNanos`    | `int`          | Required         | Initialization timestamp in nanoseconds. |

## Behavior

- Mark prices are cached by instrument when received.
- Backtests can feed mark prices to align margin and PnL behavior with venues that
  publish reference prices separately from trades.
- The catalog stores mark prices with instrument ID and price precision metadata.

## Example

```rust tab="Rust"
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::MarkPriceUpdate,
    identifiers::InstrumentId,
    types::Price,
};

let mark = MarkPriceUpdate::new(
    InstrumentId::from("BTCUSDT-PERP.BINANCE"),
    Price::from("65000.10"),
    UnixNanos::from(1_000_000_000),
    UnixNanos::from(1_000_000_100),
);
```

```python tab="Python"
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import MarkPriceUpdate
from nautilus_trader.model import Price

mark = MarkPriceUpdate(
    instrument_id=InstrumentId.from_str("BTCUSDT-PERP.BINANCE"),
    value=Price.from_str("65000.10"),
    ts_event=1_000_000_000,
    ts_init=1_000_000_100,
)
```

## Related guides

- [IndexPriceUpdate](index_price_update.md) covers the index reference price.
- [FundingRateUpdate](funding_rate_update.md) covers perpetual funding metadata.
- [Python API Reference](/docs/python-api-latest/model/data.html) lists Python members.
