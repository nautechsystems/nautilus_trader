# Futures Spread

`FuturesSpread` represents an exchange-defined futures strategy with more than one leg,
such as a calendar spread or inter-commodity spread. The venue defines the strategy,
symbol, tick size, and expiry.

Examples include listed futures calendar spreads and exchange-supported spread markets.

## Fields

| Field             | Rust type          | Python type        | Required/default | Notes                                     |
|-------------------|--------------------|--------------------|------------------|-------------------------------------------|
| `instrument_id`   | `InstrumentId`     | `InstrumentId`     | Required         | Stored as `id` in Rust.                   |
| `raw_symbol`      | `Symbol`           | `Symbol`           | Required         | Native venue symbol.                      |
| `asset_class`     | `AssetClass`       | `AssetClass`       | Required         | Asset class of the underlying strategy.   |
| `exchange`        | `Option<Ustr>`     | `str \| None`      | `None`           | Exchange MIC or venue code when known.    |
| `underlying`      | `Ustr`             | `str`              | Required         | Underlying product or product family.     |
| `strategy_type`   | `Ustr`             | `str`              | Required         | Venue strategy type, such as calendar.    |
| `activation_ns`   | `UnixNanos`        | `int`              | Required         | Strategy activation timestamp.            |
| `expiration_ns`   | `UnixNanos`        | `int`              | Required         | Strategy expiration timestamp.            |
| `currency`        | `Currency`         | `Currency`         | Required         | Quote and settlement currency.            |
| `price_precision` | `u8`               | `int`              | Required         | Decimal places allowed for prices.        |
| `price_increment` | `Price`            | `Price`            | Required         | Smallest valid price step.                |
| `size_precision`  | `u8`               | `int`              | `0`              | Futures spreads trade in whole contracts. |
| `size_increment`  | `Quantity`         | `Quantity`         | `1`              | Minimum contract size step.               |
| `multiplier`      | `Quantity`         | `Quantity`         | Required         | Strategy multiplier.                      |
| `lot_size`        | `Quantity`         | `Quantity`         | Required         | Rounded lot or contract lot size.         |
| `margin_init`     | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Initial margin rate.                      |
| `margin_maint`    | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Maintenance margin rate.                  |
| `maker_fee`       | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Maker fee rate. Negative values rebate.   |
| `taker_fee`       | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Taker fee rate. Negative values rebate.   |
| `max_quantity`    | `Option<Quantity>` | `Quantity \| None` | `None`           | Maximum order quantity.                   |
| `min_quantity`    | `Option<Quantity>` | `Quantity \| None` | `1`              | Minimum order quantity.                   |
| `max_price`       | `Option<Price>`    | `Price \| None`    | `None`           | Maximum valid quote or order price.       |
| `min_price`       | `Option<Price>`    | `Price \| None`    | `None`           | Minimum valid quote or order price.       |
| `tick_scheme`     | `Option<Ustr>`     | `str \| None`      | `None`           | Registered variable tick scheme name.     |
| `info`            | `Option<Params>`   | `dict \| None`     | `None`           | Adapter metadata.                         |
| `ts_event`        | `UnixNanos`        | `int`              | Required         | Event timestamp in nanoseconds.           |
| `ts_init`         | `UnixNanos`        | `int`              | Required         | Initialization timestamp in nanoseconds.  |

*Note: Python constructors use `instrument_id`; Rust stores the same value as `id`.*

## Behavior

- `FuturesSpread` has instrument class `FuturesSpread`.
- The venue publishes the spread as a single tradable instrument.
- It trades in whole contracts with size precision `0` and size increment `1`.
- Use leg data from the adapter metadata when a strategy needs venue-specific leg details.

## Example

```rust tab="Rust"
use chrono::{TimeZone, Utc};
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::AssetClass,
    identifiers::{InstrumentId, Symbol},
    instruments::FuturesSpread,
    types::{Currency, Price, Quantity},
};
use ustr::Ustr;

let activation = Utc.with_ymd_and_hms(2022, 6, 21, 13, 30, 0).unwrap();
let expiration = Utc.with_ymd_and_hms(2024, 6, 21, 13, 30, 0).unwrap();

let es_spread = FuturesSpread::builder()
    .instrument_id(InstrumentId::from("ESM4-ESU4.GLBX"))
    .raw_symbol(Symbol::from("ESM4-ESU4"))
    .asset_class(AssetClass::Index)
    .exchange(Ustr::from("XCME"))
    .underlying(Ustr::from("ES"))
    .strategy_type(Ustr::from("EQ"))
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
from nautilus_trader.model import FuturesSpread
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import Symbol

es_spread = FuturesSpread(
    instrument_id=InstrumentId.from_str("ESM4-ESU4.GLBX"),
    raw_symbol=Symbol("ESM4-ESU4"),
    asset_class=AssetClass.INDEX,
    underlying="ES",
    strategy_type="EQ",
    activation_ns=pd.Timestamp("2022-06-21T13:30:00", tz="UTC").value,
    expiration_ns=pd.Timestamp("2024-06-21T13:30:00", tz="UTC").value,
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

Representative adapters that create or consume `FuturesSpread` instruments include:

- [Databento](../../integrations/databento.md) for listed futures spread markets.
- [Interactive Brokers](../../integrations/ib.md) for exchange-defined futures strategies.

## Related guides

- [Futures Contract](futures_contract.md) covers single-leg futures.
- [Continuous Futures](../continuous_futures.md) covers roll-adjusted futures series.
