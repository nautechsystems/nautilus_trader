# IndexPriceUpdate

`IndexPriceUpdate` represents the external index price used by a derivatives market.
Venues often derive mark prices, funding calculations, or settlement behavior from
an index price.

## Fields

| Field           | Rust type      | Python type    | Required/default | Notes                                    |
|-----------------|----------------|----------------|------------------|------------------------------------------|
| `instrument_id` | `InstrumentId` | `InstrumentId` | Required         | Instrument for the index price.          |
| `value`         | `Price`        | `Price`        | Required         | Current index price.                     |
| `ts_event`      | `UnixNanos`    | `int`          | Required         | Event timestamp in nanoseconds.          |
| `ts_init`       | `UnixNanos`    | `int`          | Required         | Initialization timestamp in nanoseconds. |

## Behavior

- Index prices are reference data and do not imply a trade occurred.
- Perpetual and futures venues may publish both mark and index prices.
- The catalog stores index prices with instrument ID and price precision metadata.

## Example

```rust tab="Rust"
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::IndexPriceUpdate,
    identifiers::InstrumentId,
    types::Price,
};

let index = IndexPriceUpdate::new(
    InstrumentId::from("BTCUSDT-PERP.BINANCE"),
    Price::from("64995.50"),
    UnixNanos::from(1_000_000_000),
    UnixNanos::from(1_000_000_100),
);
```

```python tab="Python"
from nautilus_trader.model import IndexPriceUpdate
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price

index = IndexPriceUpdate(
    instrument_id=InstrumentId.from_str("BTCUSDT-PERP.BINANCE"),
    value=Price.from_str("64995.50"),
    ts_event=1_000_000_000,
    ts_init=1_000_000_100,
)
```

## Related guides

- [MarkPriceUpdate](mark_price_update.md) covers mark prices.
- [FundingRateUpdate](funding_rate_update.md) covers perpetual funding metadata.
- [Python API Reference](/docs/python-api-latest/model/data.html) lists Python members.
