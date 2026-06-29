# Crypto Option Spread

`CryptoOptionSpread` represents an exchange-defined strategy over crypto options. The
venue publishes the strategy as one instrument with its own symbol, strategy type,
precision, increments, and expiration.

Examples include listed BTC or ETH option combos on crypto derivatives venues.

## Fields

<Tabs items={["Rust", "Python"]}>
<Tab value="Rust">

| Field                 | Type               | Required/default | Notes                                    |
|-----------------------|--------------------|------------------|------------------------------------------|
| `instrument_id`       | `InstrumentId`     | Required         | Stored as `id` in Rust.                  |
| `raw_symbol`          | `Symbol`           | Required         | Native venue symbol.                     |
| `underlying`          | `Currency`         | Required         | Crypto asset the strategy tracks.        |
| `quote_currency`      | `Currency`         | Required         | Currency used to quote the premium.      |
| `settlement_currency` | `Currency`         | Required         | Currency used to settle PnL and fees.    |
| `is_inverse`          | `bool`             | Required         | True when sizing/costing is inverse.     |
| `strategy_type`       | `Ustr`             | Required         | Venue strategy type, such as vertical.   |
| `activation_ns`       | `UnixNanos`        | Required         | Strategy activation timestamp.           |
| `expiration_ns`       | `UnixNanos`        | Required         | Strategy expiration timestamp.           |
| `price_precision`     | `u8`               | Required         | Decimal places allowed for prices.       |
| `size_precision`      | `u8`               | Required         | Decimal places allowed for order sizes.  |
| `price_increment`     | `Price`            | Required         | Smallest valid price step.               |
| `size_increment`      | `Quantity`         | Required         | Smallest valid size step.                |
| `multiplier`          | `Quantity`         | `1`              | Strategy multiplier.                     |
| `lot_size`            | `Quantity`         | `1`              | Rounded lot or board size.               |
| `max_quantity`        | `Option<Quantity>` | `None`           | Maximum order quantity.                  |
| `min_quantity`        | `Option<Quantity>` | `None`           | Minimum order quantity.                  |
| `max_notional`        | `Option<Money>`    | `None`           | Maximum order notional value.            |
| `min_notional`        | `Option<Money>`    | `None`           | Minimum order notional value.            |
| `max_price`           | `Option<Price>`    | `None`           | Maximum valid quote or order price.      |
| `min_price`           | `Option<Price>`    | `None`           | Minimum valid quote or order price.      |
| `margin_init`         | `Option<Decimal>`  | `0`              | Initial margin rate.                     |
| `margin_maint`        | `Option<Decimal>`  | `0`              | Maintenance margin rate.                 |
| `maker_fee`           | `Option<Decimal>`  | `0`              | Maker fee rate. Negative values rebate.  |
| `taker_fee`           | `Option<Decimal>`  | `0`              | Taker fee rate. Negative values rebate.  |
| `tick_scheme`         | `Option<Ustr>`     | `None`           | Registered variable tick scheme name.    |
| `info`                | `Option<Params>`   | `None`           | Adapter metadata.                        |
| `ts_event`            | `UnixNanos`        | Required         | Event timestamp in nanoseconds.          |
| `ts_init`             | `UnixNanos`        | Required         | Initialization timestamp in nanoseconds. |

</Tab>
<Tab value="Python">

| Field                 | Type               | Required/default | Notes                                    |
|-----------------------|--------------------|------------------|------------------------------------------|
| `instrument_id`       | `InstrumentId`     | Required         |                                          |
| `raw_symbol`          | `Symbol`           | Required         | Native venue symbol.                     |
| `underlying`          | `Currency`         | Required         | Crypto asset the strategy tracks.        |
| `quote_currency`      | `Currency`         | Required         | Currency used to quote the premium.      |
| `settlement_currency` | `Currency`         | Required         | Currency used to settle PnL and fees.    |
| `is_inverse`          | `bool`             | Required         | True when sizing/costing is inverse.     |
| `strategy_type`       | `str`              | Required         | Venue strategy type, such as vertical.   |
| `activation_ns`       | `int`              | Required         | Strategy activation timestamp.           |
| `expiration_ns`       | `int`              | Required         | Strategy expiration timestamp.           |
| `price_precision`     | `int`              | Required         | Decimal places allowed for prices.       |
| `size_precision`      | `int`              | Required         | Decimal places allowed for order sizes.  |
| `price_increment`     | `Price`            | Required         | Smallest valid price step.               |
| `size_increment`      | `Quantity`         | Required         | Smallest valid size step.                |
| `multiplier`          | `Quantity`         | `1`              | Strategy multiplier.                     |
| `lot_size`            | `Quantity`         | `1`              | Rounded lot or board size.               |
| `max_quantity`        | `Quantity \| None` | `None`           | Maximum order quantity.                  |
| `min_quantity`        | `Quantity \| None` | `None`           | Minimum order quantity.                  |
| `max_notional`        | `Money \| None`    | `None`           | Maximum order notional value.            |
| `min_notional`        | `Money \| None`    | `None`           | Minimum order notional value.            |
| `max_price`           | `Price \| None`    | `None`           | Maximum valid quote or order price.      |
| `min_price`           | `Price \| None`    | `None`           | Minimum valid quote or order price.      |
| `margin_init`         | `Decimal \| None`  | `0`              | Initial margin rate.                     |
| `margin_maint`        | `Decimal \| None`  | `0`              | Maintenance margin rate.                 |
| `maker_fee`           | `Decimal \| None`  | `0`              | Maker fee rate. Negative values rebate.  |
| `taker_fee`           | `Decimal \| None`  | `0`              | Taker fee rate. Negative values rebate.  |
| `tick_scheme`         | `str \| None`      | `None`           | Registered variable tick scheme name.    |
| `info`                | `dict \| None`     | `None`           | Adapter metadata.                        |
| `ts_event`            | `int`              | Required         | Event timestamp in nanoseconds.          |
| `ts_init`             | `int`              | Required         | Initialization timestamp in nanoseconds. |

</Tab>
</Tabs>

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

let btc_spread = CryptoOptionSpread::builder()
    .instrument_id(InstrumentId::from("BTC-CS-19MAY26-70000_75000.DERIBIT"))
    .raw_symbol(Symbol::from("BTC-CS-19MAY26-70000_75000"))
    .underlying(Currency::from("BTC"))
    .quote_currency(Currency::from("USD"))
    .settlement_currency(Currency::from("BTC"))
    .is_inverse(false)
    .strategy_type(Ustr::from("CS"))
    .activation_ns(UnixNanos::from(activation.timestamp_nanos_opt().unwrap() as u64))
    .expiration_ns(UnixNanos::from(expiration.timestamp_nanos_opt().unwrap() as u64))
    .price_precision(4)
    .size_precision(1)
    .price_increment(Price::from("0.0001"))
    .size_increment(Quantity::from("0.1"))
    .multiplier(Quantity::from("1"))
    .min_quantity(Quantity::from("0.1"))
    .maker_fee(dec!(0.0003))
    .taker_fee(dec!(0.0003))
    .ts_event(UnixNanos::default())
    .ts_init(UnixNanos::default())
    .build()
    .unwrap();
```

```python tab="Python"
from decimal import Decimal

import pandas as pd

from nautilus_trader.model import CryptoOptionSpread
from nautilus_trader.model import Currency
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import Symbol

BTC = Currency.from_str("BTC")
USD = Currency.from_str("USD")

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
