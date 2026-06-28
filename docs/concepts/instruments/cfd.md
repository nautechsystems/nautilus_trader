# Cfd

`Cfd` represents a contract for difference that tracks an underlying asset without
transferring ownership of the underlying. The venue defines the quote currency,
precision, increments, limits, margins, and fees.

Examples include CFD contracts on FX, equities, indexes, and commodities.

## Fields

<Tabs items={["Rust", "Python"]}>
<Tab value="Rust">

| Field             | Type               | Required/default | Notes                                    |
|-------------------|--------------------|------------------|------------------------------------------|
| `instrument_id`   | `InstrumentId`     | Required         | Stored as `id` in Rust.                  |
| `raw_symbol`      | `Symbol`           | Required         | Native venue symbol.                     |
| `asset_class`     | `AssetClass`       | Required         | Asset class of the underlying.           |
| `base_currency`   | `Option<Currency>` | `None`           | Base currency when the CFD tracks one.   |
| `quote_currency`  | `Currency`         | Required         | Currency used to quote and value prices. |
| `price_precision` | `u8`               | Required         | Decimal places allowed for prices.       |
| `size_precision`  | `u8`               | Required         | Decimal places allowed for order sizes.  |
| `price_increment` | `Price`            | Required         | Smallest valid price step.               |
| `size_increment`  | `Quantity`         | Required         | Smallest valid size step.                |
| `lot_size`        | `Option<Quantity>` | `None`           | Rounded lot or board size.               |
| `max_quantity`    | `Option<Quantity>` | `None`           | Maximum order quantity.                  |
| `min_quantity`    | `Option<Quantity>` | `None`           | Minimum order quantity.                  |
| `max_notional`    | `Option<Money>`    | `None`           | Maximum order notional value.            |
| `min_notional`    | `Option<Money>`    | `None`           | Minimum order notional value.            |
| `max_price`       | `Option<Price>`    | `None`           | Maximum valid quote or order price.      |
| `min_price`       | `Option<Price>`    | `None`           | Minimum valid quote or order price.      |
| `margin_init`     | `Option<Decimal>`  | `0`              | Initial margin rate.                     |
| `margin_maint`    | `Option<Decimal>`  | `0`              | Maintenance margin rate.                 |
| `maker_fee`       | `Option<Decimal>`  | `0`              | Maker fee rate. Negative values rebate.  |
| `taker_fee`       | `Option<Decimal>`  | `0`              | Taker fee rate. Negative values rebate.  |
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
| `base_currency`   | `Currency \| None` | `None`           | Base currency when the CFD tracks one.   |
| `quote_currency`  | `Currency`         | Required         | Currency used to quote and value prices. |
| `price_precision` | `int`              | Required         | Decimal places allowed for prices.       |
| `size_precision`  | `int`              | Required         | Decimal places allowed for order sizes.  |
| `price_increment` | `Price`            | Required         | Smallest valid price step.               |
| `size_increment`  | `Quantity`         | Required         | Smallest valid size step.                |
| `lot_size`        | `Quantity \| None` | `None`           | Rounded lot or board size.               |
| `max_quantity`    | `Quantity \| None` | `None`           | Maximum order quantity.                  |
| `min_quantity`    | `Quantity \| None` | `None`           | Minimum order quantity.                  |
| `max_notional`    | `Money \| None`    | `None`           | Maximum order notional value.            |
| `min_notional`    | `Money \| None`    | `None`           | Minimum order notional value.            |
| `max_price`       | `Price \| None`    | `None`           | Maximum valid quote or order price.      |
| `min_price`       | `Price \| None`    | `None`           | Minimum valid quote or order price.      |
| `margin_init`     | `Decimal \| None`  | `0`              | Initial margin rate.                     |
| `margin_maint`    | `Decimal \| None`  | `0`              | Maintenance margin rate.                 |
| `maker_fee`       | `Decimal \| None`  | `0`              | Maker fee rate. Negative values rebate.  |
| `taker_fee`       | `Decimal \| None`  | `0`              | Taker fee rate. Negative values rebate.  |
| `tick_scheme`     | `str \| None`      | `None`           | Registered variable tick scheme name.    |
| `info`            | `dict \| None`     | `None`           | Adapter metadata.                        |
| `ts_event`        | `int`              | Required         | Event timestamp in nanoseconds.          |
| `ts_init`         | `int`              | Required         | Initialization timestamp in nanoseconds. |

</Tab>
</Tabs>

*Note: Python constructors use `instrument_id`; Rust stores the same value as `id`.*

## Behavior

- `Cfd` has instrument class `Cfd`.
- It is never inverse and uses a multiplier of one.
- It has no activation timestamp, expiration timestamp, strike, or option kind.
- Use the source market type when a venue offers both cash instruments and CFDs.

## Example

```rust tab="Rust"
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::AssetClass,
    identifiers::{InstrumentId, Symbol},
    instruments::Cfd,
    types::{Currency, Price, Quantity},
};
use rust_decimal_macros::dec;

let audusd = Cfd::builder()
    .instrument_id(InstrumentId::from("AUDUSD.OANDA"))
    .raw_symbol(Symbol::from("AUD/USD"))
    .asset_class(AssetClass::FX)
    .base_currency(Currency::from("AUD"))
    .quote_currency(Currency::from("USD"))
    .price_precision(5)
    .size_precision(0)
    .price_increment(Price::from("0.00001"))
    .size_increment(Quantity::from("1"))
    .lot_size(Quantity::from("1000"))
    .margin_init(dec!(0.03))
    .margin_maint(dec!(0.03))
    .maker_fee(dec!(0.00002))
    .taker_fee(dec!(0.00002))
    .ts_event(UnixNanos::default())
    .ts_init(UnixNanos::default())
    .build()
    .unwrap();
```

```python tab="Python"
from decimal import Decimal

from nautilus_trader.model import AssetClass
from nautilus_trader.model import Cfd
from nautilus_trader.model import Currency
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import Symbol

audusd = Cfd(
    instrument_id=InstrumentId.from_str("AUDUSD.OANDA"),
    raw_symbol=Symbol("AUD/USD"),
    asset_class=AssetClass.FX,
    quote_currency=Currency.from_str("USD"),
    price_precision=5,
    price_increment=Price.from_str("0.00001"),
    size_precision=0,
    size_increment=Quantity.from_int(1),
    ts_event=0,
    ts_init=0,
    base_currency=Currency.from_str("AUD"),
    lot_size=Quantity.from_int(1000),
    margin_init=Decimal("0.03"),
    margin_maint=Decimal("0.03"),
    maker_fee=Decimal("0.00002"),
    taker_fee=Decimal("0.00002"),
)
```

## Adapters

Representative adapters that create or consume `Cfd` instruments include:

- [Interactive Brokers](../../integrations/ib.md) for CFD contracts.

## Related guides

- [Currency Pair](currency_pair.md) covers cash FX and crypto spot pairs.
- [Commodity](commodity.md) covers spot commodity instruments.
