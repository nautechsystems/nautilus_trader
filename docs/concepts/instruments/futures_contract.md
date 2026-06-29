# Futures Contract

`FuturesContract` represents a dated, exchange-traded futures contract with a defined
underlying, activation time, expiration time, currency, multiplier, and lot size.

Examples include equity index futures, commodity futures, interest-rate futures, and
currency futures.

## Fields

<Tabs items={["Rust", "Python"]}>
<Tab value="Rust">

| Field             | Type               | Required/default | Notes                                    |
|-------------------|--------------------|------------------|------------------------------------------|
| `instrument_id`   | `InstrumentId`     | Required         | Stored as `id` in Rust.                  |
| `raw_symbol`      | `Symbol`           | Required         | Native venue symbol.                     |
| `asset_class`     | `AssetClass`       | Required         | Asset class of the underlying.           |
| `exchange`        | `Option<Ustr>`     | `None`           | Exchange MIC or venue code when known.   |
| `underlying`      | `Ustr`             | Required         | Underlying asset, index, or product.     |
| `activation_ns`   | `UnixNanos`        | Required         | Contract activation timestamp.           |
| `expiration_ns`   | `UnixNanos`        | Required         | Contract expiration timestamp.           |
| `currency`        | `Currency`         | Required         | Quote and settlement currency.           |
| `price_precision` | `u8`               | Required         | Decimal places allowed for prices.       |
| `price_increment` | `Price`            | Required         | Smallest valid price step.               |
| `size_precision`  | `u8`               | `0`              | Futures trade in whole contracts.        |
| `size_increment`  | `Quantity`         | `1`              | Minimum contract size step.              |
| `multiplier`      | `Quantity`         | Required         | Contract multiplier.                     |
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
| `asset_class`     | `AssetClass`       | Required         | Asset class of the underlying.           |
| `exchange`        | `str \| None`      | `None`           | Exchange MIC or venue code when known.   |
| `underlying`      | `str`              | Required         | Underlying asset, index, or product.     |
| `activation_ns`   | `int`              | Required         | Contract activation timestamp.           |
| `expiration_ns`   | `int`              | Required         | Contract expiration timestamp.           |
| `currency`        | `Currency`         | Required         | Quote and settlement currency.           |
| `price_precision` | `int`              | Required         | Decimal places allowed for prices.       |
| `price_increment` | `Price`            | Required         | Smallest valid price step.               |
| `size_precision`  | `int`              | `0`              | Futures trade in whole contracts.        |
| `size_increment`  | `Quantity`         | `1`              | Minimum contract size step.              |
| `multiplier`      | `Quantity`         | Required         | Contract multiplier.                     |
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

- `FuturesContract` has instrument class `Future`.
- It is never inverse. Cost, settlement, and quote currency use `currency`.
- It trades in whole contracts with size precision `0` and size increment `1`.
- Use `CryptoFuture` for dated crypto futures where the underlying and settlement
  currencies can differ.

## Example

```rust tab="Rust"
use chrono::{TimeZone, Utc};
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::AssetClass,
    identifiers::{InstrumentId, Symbol},
    instruments::FuturesContract,
    types::{Currency, Price, Quantity},
};
use ustr::Ustr;

let activation = Utc.with_ymd_and_hms(2021, 9, 10, 0, 0, 0).unwrap();
let expiration = Utc.with_ymd_and_hms(2021, 12, 17, 0, 0, 0).unwrap();

let esz21 = FuturesContract::builder()
    .instrument_id(InstrumentId::from("ESZ21.GLBX"))
    .raw_symbol(Symbol::from("ESZ21"))
    .asset_class(AssetClass::Index)
    .exchange(Ustr::from("XCME"))
    .underlying(Ustr::from("ES"))
    .activation_ns(UnixNanos::from(activation.timestamp_nanos_opt().unwrap() as u64))
    .expiration_ns(UnixNanos::from(expiration.timestamp_nanos_opt().unwrap() as u64))
    .currency(Currency::from("USD"))
    .price_precision(2)
    .price_increment(Price::from("0.25"))
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
from nautilus_trader.model import FuturesContract
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import Symbol

esz21 = FuturesContract(
    instrument_id=InstrumentId.from_str("ESZ21.GLBX"),
    raw_symbol=Symbol("ESZ21"),
    asset_class=AssetClass.INDEX,
    underlying="ES",
    activation_ns=pd.Timestamp("2021-09-10", tz="UTC").value,
    expiration_ns=pd.Timestamp("2021-12-17", tz="UTC").value,
    currency=Currency.from_str("USD"),
    price_precision=2,
    price_increment=Price.from_str("0.25"),
    multiplier=Quantity.from_int(1),
    lot_size=Quantity.from_int(1),
    ts_event=0,
    ts_init=0,
    exchange="XCME",
)
```

## Adapters

Representative adapters that create or consume `FuturesContract` instruments include:

- [Databento](../../integrations/databento.md) for futures reference data and market data.
- [Interactive Brokers](../../integrations/ib.md) for listed futures contracts.

## Related guides

- [Continuous Futures](../continuous_futures.md) covers roll-adjusted futures series.
- [Crypto Future](crypto_future.md) covers dated crypto futures contracts.
