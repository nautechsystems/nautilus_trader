# Option Spread

`OptionSpread` represents an exchange-defined options strategy with more than one leg.
The venue publishes the strategy as a single instrument with its own symbol, tick size,
expiration, and execution rules.

Examples include listed vertical spreads, calendar spreads, and other option strategies.

## Fields

| Field             | Rust type          | Python type        | Required/default | Notes                                    |
|-------------------|--------------------|--------------------|------------------|------------------------------------------|
| `instrument_id`   | `InstrumentId`     | `InstrumentId`     | Required         | Stored as `id` in Rust.                  |
| `raw_symbol`      | `Symbol`           | `Symbol`           | Required         | Native venue symbol.                     |
| `asset_class`     | `AssetClass`       | `AssetClass`       | Required         | Asset class of the underlying strategy.  |
| `exchange`        | `Option<Ustr>`     | `str \| None`      | `None`           | Exchange MIC or venue code when known.   |
| `underlying`      | `Ustr`             | `str`              | Required         | Underlying asset, future, or index.      |
| `strategy_type`   | `Ustr`             | `str`              | Required         | Venue strategy type, such as vertical.   |
| `activation_ns`   | `UnixNanos`        | `int`              | Required         | Strategy activation timestamp.           |
| `expiration_ns`   | `UnixNanos`        | `int`              | Required         | Strategy expiration timestamp.           |
| `currency`        | `Currency`         | `Currency`         | Required         | Premium quote and settlement currency.   |
| `price_precision` | `u8`               | `int`              | Required         | Decimal places allowed for prices.       |
| `price_increment` | `Price`            | `Price`            | Required         | Smallest valid price step.               |
| `size_precision`  | `u8`               | `int`              | `0`              | Option spreads trade in whole contracts. |
| `size_increment`  | `Quantity`         | `Quantity`         | `1`              | Minimum contract size step.              |
| `multiplier`      | `Quantity`         | `Quantity`         | Required         | Strategy multiplier.                     |
| `lot_size`        | `Quantity`         | `Quantity`         | Required         | Rounded lot or contract lot size.        |
| `margin_init`     | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Initial margin rate.                     |
| `margin_maint`    | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Maintenance margin rate.                 |
| `maker_fee`       | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Maker fee rate. Negative values rebate.  |
| `taker_fee`       | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Taker fee rate. Negative values rebate.  |
| `max_quantity`    | `Option<Quantity>` | `Quantity \| None` | `None`           | Maximum order quantity.                  |
| `min_quantity`    | `Option<Quantity>` | `Quantity \| None` | `1`              | Minimum order quantity.                  |
| `max_price`       | `Option<Price>`    | `Price \| None`    | `None`           | Maximum valid quote or order price.      |
| `min_price`       | `Option<Price>`    | `Price \| None`    | `None`           | Minimum valid quote or order price.      |
| `tick_scheme`     | `Option<Ustr>`     | `str \| None`      | `None`           | Registered variable tick scheme name.    |
| `info`            | `Option<Params>`   | `dict \| None`     | `None`           | Adapter metadata.                        |
| `ts_event`        | `UnixNanos`        | `int`              | Required         | Event timestamp in nanoseconds.          |
| `ts_init`         | `UnixNanos`        | `int`              | Required         | Initialization timestamp in nanoseconds. |

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
