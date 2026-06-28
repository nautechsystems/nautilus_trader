# Option Spread

`OptionSpread` represents an exchange-defined options strategy with more than one leg.
The venue publishes the strategy as a single instrument with its own symbol, tick size,
expiration, and execution rules.

Examples include listed vertical spreads, calendar spreads, and other option strategies.

## Fields

<Tabs items={["Rust", "Python"]}>
<Tab value="Rust">

| Field             | Type               | Required/default | Notes                                    |
|-------------------|--------------------|------------------|------------------------------------------|
| `instrument_id`   | `InstrumentId`     | Required         | Stored as `id` in Rust.                  |
| `raw_symbol`      | `Symbol`           | Required         | Native venue symbol.                     |
| `asset_class`     | `AssetClass`       | Required         | Asset class of the underlying strategy.  |
| `exchange`        | `Option<Ustr>`     | `None`           | Exchange MIC or venue code when known.   |
| `underlying`      | `Ustr`             | Required         | Underlying asset, future, or index.      |
| `strategy_type`   | `Ustr`             | Required         | Venue strategy type, such as vertical.   |
| `activation_ns`   | `UnixNanos`        | Required         | Strategy activation timestamp.           |
| `expiration_ns`   | `UnixNanos`        | Required         | Strategy expiration timestamp.           |
| `currency`        | `Currency`         | Required         | Premium quote and settlement currency.   |
| `price_precision` | `u8`               | Required         | Decimal places allowed for prices.       |
| `price_increment` | `Price`            | Required         | Smallest valid price step.               |
| `size_precision`  | `u8`               | `0`              | Option spreads trade in whole contracts. |
| `size_increment`  | `Quantity`         | `1`              | Minimum contract size step.              |
| `multiplier`      | `Quantity`         | Required         | Strategy multiplier.                     |
| `lot_size`        | `Quantity`         | Required         | Rounded lot or contract lot size.        |
| `margin_init`     | `Option<Decimal>`  | `0`              | Initial margin rate.                     |
| `margin_maint`    | `Option<Decimal>`  | `0`              | Maintenance margin rate.                 |
| `maker_fee`       | `Option<Decimal>`  | `0`              | Maker fee rate. Negative values rebate.  |
| `taker_fee`       | `Option<Decimal>`  | `0`              | Taker fee rate. Negative values rebate.  |
| `max_quantity`    | `Option<Quantity>` | `None`           | Maximum order quantity.                  |
| `min_quantity`    | `Option<Quantity>` | `1`              | Minimum order quantity.                  |
| `max_price`       | `Option<Price>`    | `None`           | Maximum valid quote or order price.      |
| `min_price`       | `Option<Price>`    | `None`           | Minimum valid quote or order price.      |
| `tick_scheme`     | `Option<Ustr>`     | `None`           | Registered variable tick scheme name.    |
| `info`            | `Option<Params>`   | `None`           | Adapter metadata.                        |
| `ts_event`        | `UnixNanos`        | Required         | Event timestamp in nanoseconds.          |
| `ts_init`         | `UnixNanos`        | Required         | Initialization timestamp in nanoseconds. |

</Tab>
<Tab value="Python">

| Field             | Type               | Required/default | Notes                                    |
|-------------------|--------------------|------------------|------------------------------------------|
| `instrument_id`   | `InstrumentId`     | Required         |                                          |
| `raw_symbol`      | `Symbol`           | Required         | Native venue symbol.                     |
| `asset_class`     | `AssetClass`       | Required         | Asset class of the underlying strategy.  |
| `exchange`        | `str \| None`      | `None`           | Exchange MIC or venue code when known.   |
| `underlying`      | `str`              | Required         | Underlying asset, future, or index.      |
| `strategy_type`   | `str`              | Required         | Venue strategy type, such as vertical.   |
| `activation_ns`   | `int`              | Required         | Strategy activation timestamp.           |
| `expiration_ns`   | `int`              | Required         | Strategy expiration timestamp.           |
| `currency`        | `Currency`         | Required         | Premium quote and settlement currency.   |
| `price_precision` | `int`              | Required         | Decimal places allowed for prices.       |
| `price_increment` | `Price`            | Required         | Smallest valid price step.               |
| `size_precision`  | `int`              | `0`              | Option spreads trade in whole contracts. |
| `size_increment`  | `Quantity`         | `1`              | Minimum contract size step.              |
| `multiplier`      | `Quantity`         | Required         | Strategy multiplier.                     |
| `lot_size`        | `Quantity`         | Required         | Rounded lot or contract lot size.        |
| `margin_init`     | `Decimal \| None`  | `0`              | Initial margin rate.                     |
| `margin_maint`    | `Decimal \| None`  | `0`              | Maintenance margin rate.                 |
| `maker_fee`       | `Decimal \| None`  | `0`              | Maker fee rate. Negative values rebate.  |
| `taker_fee`       | `Decimal \| None`  | `0`              | Taker fee rate. Negative values rebate.  |
| `max_quantity`    | `Quantity \| None` | `None`           | Maximum order quantity.                  |
| `min_quantity`    | `Quantity \| None` | `1`              | Minimum order quantity.                  |
| `max_price`       | `Price \| None`    | `None`           | Maximum valid quote or order price.      |
| `min_price`       | `Price \| None`    | `None`           | Minimum valid quote or order price.      |
| `tick_scheme`     | `str \| None`      | `None`           | Registered variable tick scheme name.    |
| `info`            | `dict \| None`     | `None`           | Adapter metadata.                        |
| `ts_event`        | `int`              | Required         | Event timestamp in nanoseconds.          |
| `ts_init`         | `int`              | Required         | Initialization timestamp in nanoseconds. |

</Tab>
</Tabs>

*Note: Python constructors use `instrument_id`; Rust stores the same value as `id`.*

## Behavior

- `OptionSpread` has instrument class `OptionSpread`.
- The venue publishes the spread as a single tradable instrument.
- It trades in whole contracts with size precision `0` and size increment `1`.
- Store venue-specific leg details in `info` when the adapter provides them.

## Example

```rust tab="Rust"
use chrono::{TimeZone, Utc};
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::AssetClass,
    identifiers::{InstrumentId, Symbol},
    instruments::OptionSpread,
    types::{Currency, Price, Quantity},
};
use ustr::Ustr;

let activation = Utc.with_ymd_and_hms(2023, 11, 6, 20, 54, 7).unwrap();
let expiration = Utc.with_ymd_and_hms(2024, 2, 23, 22, 59, 0).unwrap();

let sr3_spread = OptionSpread::builder()
    .instrument_id(InstrumentId::from("UD:U$: GN 2534559.GLBX"))
    .raw_symbol(Symbol::from("UD:U$: GN 2534559"))
    .asset_class(AssetClass::FX)
    .exchange(Ustr::from("XCME"))
    .underlying(Ustr::from("SR3"))
    .strategy_type(Ustr::from("GN"))
    .activation_ns(UnixNanos::from(activation.timestamp_nanos_opt().unwrap() as u64))
    .expiration_ns(UnixNanos::from(expiration.timestamp_nanos_opt().unwrap() as u64))
    .currency(Currency::from("USD"))
    .price_precision(2)
    .price_increment(Price::from("0.01"))
    .multiplier(Quantity::from("1"))
    .lot_size(Quantity::from("1"))
    .ts_event(UnixNanos::default())
    .ts_init(UnixNanos::default())
    .build()
    .unwrap();
```

```python tab="Python"
import pandas as pd

from nautilus_trader.model import AssetClass
from nautilus_trader.model import Currency
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OptionSpread
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import Symbol

sr3_spread = OptionSpread(
    instrument_id=InstrumentId.from_str("UD:U$: GN 2534559.GLBX"),
    raw_symbol=Symbol("UD:U$: GN 2534559"),
    asset_class=AssetClass.FX,
    underlying="SR3",
    strategy_type="GN",
    activation_ns=pd.Timestamp("2023-11-06T20:54:07", tz="UTC").value,
    expiration_ns=pd.Timestamp("2024-02-23T22:59:00", tz="UTC").value,
    currency=Currency.from_str("USD"),
    price_precision=2,
    price_increment=Price.from_str("0.01"),
    multiplier=Quantity.from_int(1),
    lot_size=Quantity.from_int(1),
    ts_event=0,
    ts_init=0,
    exchange="XCME",
)
```

## Adapters

Representative adapters that create or consume `OptionSpread` instruments include:

- [Databento](../../integrations/databento.md) for listed option spread markets.
- [Interactive Brokers](../../integrations/ib.md) for exchange-defined option strategies.

## Related guides

- [Option Contract](option_contract.md) covers single-leg option contracts.
- [Options](../options.md) covers option data, Greeks, and chain subscriptions.
