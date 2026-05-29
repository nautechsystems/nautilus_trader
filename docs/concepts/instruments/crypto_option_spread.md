# Crypto Option Spread

`CryptoOptionSpread` represents an exchange-defined strategy over crypto options. The
venue publishes the strategy as one instrument with its own symbol, strategy type,
precision, increments, and expiration.

Examples include listed BTC or ETH option combos on crypto derivatives venues.

## Fields

| Field                 | Rust type          | Python type       | Required/default | Notes                                   |
|-----------------------|--------------------|-------------------|------------------|-----------------------------------------|
| `instrument_id`       | `InstrumentId`     | `InstrumentId`    | Required         | Stored as `id` in Rust.                 |
| `raw_symbol`          | `Symbol`           | `Symbol`          | Required         | Native venue symbol.                    |
| `underlying`          | `Currency`         | `Currency`        | Required         | Crypto asset the strategy tracks.       |
| `quote_currency`      | `Currency`         | `Currency`        | Required         | Currency used to quote the premium.     |
| `settlement_currency` | `Currency`         | `Currency`        | Required         | Currency used to settle PnL and fees.   |
| `is_inverse`          | `bool`             | `bool`            | Required         | True when sizing/costing is inverse.    |
| `strategy_type`       | `Ustr`             | `str`             | Required         | Venue strategy type, such as vertical.  |
| `activation_ns`       | `UnixNanos`        | `int`             | Required         | Strategy activation timestamp.          |
| `expiration_ns`       | `UnixNanos`        | `int`             | Required         | Strategy expiration timestamp.          |
| `price_precision`     | `u8`               | `int`             | Required         | Decimal places allowed for prices.      |
| `size_precision`      | `u8`               | `int`             | Required         | Decimal places allowed for order sizes. |
| `price_increment`     | `Price`            | `Price`           | Required         | Smallest valid price step.              |
| `size_increment`      | `Quantity`         | `Quantity`        | Required         | Smallest valid size step.               |
| `multiplier`          | `Quantity`         | `Quantity`        | `1`              | Strategy multiplier.                    |
| `lot_size`            | `Quantity`         | `Quantity`        | `1`              | Rounded lot or board size.              |
| `max_quantity`        | `Option<Quantity>` | `Quantity \| None` | `None`           | Maximum order quantity.                 |
| `min_quantity`        | `Option<Quantity>` | `Quantity \| None` | `None`           | Minimum order quantity.                 |
| `max_notional`        | `Option<Money>`    | `Money \| None`    | `None`           | Maximum order notional value.           |
| `min_notional`        | `Option<Money>`    | `Money \| None`    | `None`           | Minimum order notional value.           |
| `max_price`           | `Option<Price>`    | `Price \| None`    | `None`           | Maximum valid quote or order price.     |
| `min_price`           | `Option<Price>`    | `Price \| None`    | `None`           | Minimum valid quote or order price.     |
| `margin_init`         | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Initial margin rate.                    |
| `margin_maint`        | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Maintenance margin rate.                |
| `maker_fee`           | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Maker fee rate. Negative values rebate. |
| `taker_fee`           | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Taker fee rate. Negative values rebate. |
| `tick_scheme_name`    | N/A                | `str \| None`      | `None`           | Registered variable tick scheme name.   |
| `info`                | `Option<Params>`   | `dict \| None`     | `None`           | Adapter metadata.                       |
| `ts_event`            | `UnixNanos`        | `int`             | Required         | Event timestamp in nanoseconds.         |
| `ts_init`             | `UnixNanos`        | `int`             | Required         | Initialization timestamp in nanoseconds. |

*Note: Python constructors use `instrument_id`; Rust stores the same value as `id`.*

## Behavior

- `CryptoOptionSpread` has asset class `Cryptocurrency` and instrument class
  `OptionSpread`.
- The venue publishes the spread as a single tradable instrument.
- The strategy can be linear, inverse, or quanto, depending on the currency set.
- Store venue-specific leg details in `info` when the adapter provides them.

## Example

```rust tab="Rust"
use chrono::{TimeZone, Utc};
use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::{InstrumentId, Symbol},
    instruments::CryptoOptionSpread,
    types::{Currency, Price, Quantity},
};
use rust_decimal_macros::dec;
use ustr::Ustr;

let activation = Utc.with_ymd_and_hms(2026, 5, 12, 0, 0, 0).unwrap();
let expiration = Utc.with_ymd_and_hms(2026, 5, 19, 8, 0, 0).unwrap();

let btc_spread = CryptoOptionSpread::new(
    InstrumentId::from("BTC-CS-19MAY26-70000_75000.DERIBIT"),
    Symbol::from("BTC-CS-19MAY26-70000_75000"),
    Currency::from("BTC"),
    Currency::from("USD"),
    Currency::from("BTC"),
    false,
    Ustr::from("CS"),
    UnixNanos::from(activation.timestamp_nanos_opt().unwrap() as u64),
    UnixNanos::from(expiration.timestamp_nanos_opt().unwrap() as u64),
    4,
    1,
    Price::from("0.0001"),
    Quantity::from("0.1"),
    Some(Quantity::from("1")),
    None,
    None,
    Some(Quantity::from("0.1")),
    None,
    None,
    None,
    None,
    None,
    None,
    Some(dec!(0.0003)),
    Some(dec!(0.0003)),
    None,
    UnixNanos::default(),
    UnixNanos::default(),
);
```

```python tab="Python"
from decimal import Decimal

import pandas as pd

from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import CryptoOptionSpread
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity

btc_spread = CryptoOptionSpread(
    instrument_id=InstrumentId.from_str("BTC-CS-19MAY26-70000_75000.DERIBIT"),
    raw_symbol=Symbol("BTC-CS-19MAY26-70000_75000"),
    underlying=BTC,
    quote_currency=USD,
    settlement_currency=BTC,
    is_inverse=False,
    strategy_type="CS",
    activation_ns=pd.Timestamp("2026-05-12T00:00:00", tz="UTC").value,
    expiration_ns=pd.Timestamp("2026-05-19T08:00:00", tz="UTC").value,
    price_precision=4,
    size_precision=1,
    price_increment=Price.from_str("0.0001"),
    size_increment=Quantity.from_str("0.1"),
    min_quantity=Quantity.from_str("0.1"),
    maker_fee=Decimal("0.0003"),
    taker_fee=Decimal("0.0003"),
    ts_event=0,
    ts_init=0,
)
```

## Adapters

Representative adapters that create or consume `CryptoOptionSpread` instruments include:

- [Deribit](../../integrations/deribit.md) for crypto option combos.
- [OKX](../../integrations/okx.md) for crypto option spread markets.

## Related guides

- [Crypto Option](crypto_option.md) covers single-leg crypto options.
- [Option Spread](option_spread.md) covers non-crypto option spreads.
