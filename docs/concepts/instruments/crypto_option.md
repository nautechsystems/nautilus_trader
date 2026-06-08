# Crypto Option

`CryptoOption` represents a put or call option on a crypto underlying. It defines the
option kind, strike price, activation time, expiration time, quote currency, settlement
currency, and contract sizing.

Examples include BTC and ETH options on crypto derivatives venues.

## Fields

| Field                 | Rust type          | Python type       | Required/default | Notes                                   |
|-----------------------|--------------------|-------------------|------------------|-----------------------------------------|
| `instrument_id`       | `InstrumentId`     | `InstrumentId`    | Required         | Stored as `id` in Rust.                 |
| `raw_symbol`          | `Symbol`           | `Symbol`          | Required         | Native venue symbol.                    |
| `underlying`          | `Currency`         | `Currency`        | Required         | Crypto asset the option tracks.         |
| `quote_currency`      | `Currency`         | `Currency`        | Required         | Currency used to quote the premium.     |
| `settlement_currency` | `Currency`         | `Currency`        | Required         | Currency used to settle PnL and fees.   |
| `is_inverse`          | `bool`             | `bool`            | Required         | True when sizing/costing is inverse.    |
| `option_kind`         | `OptionKind`       | `OptionKind`      | Required         | Put or call.                            |
| `strike_price`        | `Price`            | `Price`           | Required         | Option strike price.                    |
| `activation_ns`       | `UnixNanos`        | `int`             | Required         | Contract activation timestamp.          |
| `expiration_ns`       | `UnixNanos`        | `int`             | Required         | Contract expiration timestamp.          |
| `price_precision`     | `u8`               | `int`             | Required         | Decimal places allowed for prices.      |
| `size_precision`      | `u8`               | `int`             | Required         | Decimal places allowed for order sizes. |
| `price_increment`     | `Price`            | `Price`           | Required         | Smallest valid price step.              |
| `size_increment`      | `Quantity`         | `Quantity`        | Required         | Smallest valid size step.               |
| `multiplier`          | `Quantity`         | `Quantity`        | `1`              | Contract multiplier.                    |
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

- `CryptoOption` has asset class `Cryptocurrency` and instrument class `Option`.
- The option kind and strike price define the payoff shape.
- The contract can be linear, inverse, or quanto, depending on the currency set.
- Use `OptionContract` for non-crypto listed options.

## Example

```rust tab="Rust"
use chrono::{TimeZone, Utc};
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::OptionKind,
    identifiers::{InstrumentId, Symbol},
    instruments::CryptoOption,
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal_macros::dec;

let activation = Utc.with_ymd_and_hms(2022, 12, 22, 0, 0, 0).unwrap();
let expiration = Utc.with_ymd_and_hms(2023, 1, 13, 8, 0, 0).unwrap();

let btc_option = CryptoOption::new(
    InstrumentId::from("BTC-13JAN23-16000-P.DERIBIT"),
    Symbol::from("BTC-13JAN23-16000-P"),
    Currency::from("BTC"),
    Currency::from("USD"),
    Currency::from("BTC"),
    false,
    OptionKind::Put,
    Price::from("16000.00"),
    UnixNanos::from(activation.timestamp_nanos_opt().unwrap() as u64),
    UnixNanos::from(expiration.timestamp_nanos_opt().unwrap() as u64),
    2,
    1,
    Price::from("0.01"),
    Quantity::from("0.1"),
    Some(Quantity::from("1")),
    Some(Quantity::from("1")),
    Some(Quantity::from("9000")),
    Some(Quantity::from("0.1")),
    None,
    Some(Money::from("10.00 USD")),
    None,
    None,
    Some(dec!(0)),
    Some(dec!(0)),
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
from nautilus_trader.model.enums import OptionKind
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import CryptoOption
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity

btc_option = CryptoOption(
    instrument_id=InstrumentId.from_str("BTC-13JAN23-16000-P.DERIBIT"),
    raw_symbol=Symbol("BTC-13JAN23-16000-P"),
    underlying=BTC,
    quote_currency=USD,
    settlement_currency=BTC,
    is_inverse=False,
    option_kind=OptionKind.PUT,
    strike_price=Price.from_str("16000.00"),
    activation_ns=pd.Timestamp("2022-12-22", tz="UTC").value,
    expiration_ns=pd.Timestamp("2023-01-13T08:00:00", tz="UTC").value,
    price_precision=2,
    size_precision=1,
    price_increment=Price.from_str("0.01"),
    size_increment=Quantity.from_str("0.1"),
    max_quantity=Quantity.from_str("9000"),
    min_quantity=Quantity.from_str("0.1"),
    min_notional=Money(10.00, USD),
    margin_init=Decimal(0),
    margin_maint=Decimal(0),
    maker_fee=Decimal("0.0003"),
    taker_fee=Decimal("0.0003"),
    ts_event=0,
    ts_init=0,
)
```

## Adapters

Representative adapters that create or consume `CryptoOption` instruments include:

- [Bybit](../../integrations/bybit.md) for crypto options.
- [Deribit](../../integrations/deribit.md) for crypto options.
- [OKX](../../integrations/okx.md) for crypto options.
- [Tardis](../../integrations/tardis.md) for crypto option metadata.

## Related guides

- [Options](../options.md) covers option data, Greeks, and chain subscriptions.
- [Crypto Option Spread](crypto_option_spread.md) covers exchange-defined crypto option spreads.
