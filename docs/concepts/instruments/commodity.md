# Commodity

`Commodity` represents a spot commodity market such as gold, silver, oil, or another
physical asset quoted in a currency. It models a spot market, not a dated futures
contract.

Examples include `XAUUSD.IDEALPRO` and venue-specific commodity cash symbols.

## Fields

<Tabs items={["Rust", "Python"]}>
<Tab value="Rust">

| Field             | Type               | Required/default | Notes                                    |
|-------------------|--------------------|------------------|------------------------------------------|
| `instrument_id`   | `InstrumentId`     | Required         | Stored as `id` in Rust.                  |
| `raw_symbol`      | `Symbol`           | Required         | Native venue symbol.                     |
| `asset_class`     | `AssetClass`       | Required         | Commodity asset classification.          |
| `quote_currency`  | `Currency`         | Required         | Currency used to price the commodity.    |
| `price_precision` | `u8`               | Required         | Decimal places allowed for prices.       |
| `size_precision`  | `u8`               | Required         | Decimal places allowed for order sizes.  |
| `price_increment` | `Price`            | Required         | Smallest valid price step.               |
| `size_increment`  | `Quantity`         | Required         | Smallest valid size step.                |
| `ts_event`        | `UnixNanos`        | Required         | Event timestamp in nanoseconds.          |
| `ts_init`         | `UnixNanos`        | Required         | Initialization timestamp in nanoseconds. |
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

</Tab>
<Tab value="Python">

| Field             | Type               | Required/default | Notes                                    |
|-------------------|--------------------|------------------|------------------------------------------|
| `instrument_id`   | `InstrumentId`     | Required         |                                          |
| `raw_symbol`      | `Symbol`           | Required         | Native venue symbol.                     |
| `asset_class`     | `AssetClass`       | Required         | Commodity asset classification.          |
| `quote_currency`  | `Currency`         | Required         | Currency used to price the commodity.    |
| `price_precision` | `int`              | Required         | Decimal places allowed for prices.       |
| `size_precision`  | `int`              | Required         | Decimal places allowed for order sizes.  |
| `price_increment` | `Price`            | Required         | Smallest valid price step.               |
| `size_increment`  | `Quantity`         | Required         | Smallest valid size step.                |
| `ts_event`        | `int`              | Required         | Event timestamp in nanoseconds.          |
| `ts_init`         | `int`              | Required         | Initialization timestamp in nanoseconds. |
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

</Tab>
</Tabs>

*Note: Python constructors use `instrument_id`; Rust stores the same value as `id`.*

## Behavior

- `Commodity` has instrument class `Spot`.
- It allows negative prices: spot markets such as electricity or oil can trade below zero,
  and the `RiskEngine` accepts negative prices on both order submission and modification.
- It is never inverse, and its cost currency is the quote currency.
- It has no activation timestamp, expiry, strike, option kind, or settlement currency field.
- Use `FuturesContract` for dated exchange-traded commodity futures.

## Example

```rust tab="Rust"
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::AssetClass,
    identifiers::{InstrumentId, Symbol},
    instruments::Commodity,
    types::{Currency, Price, Quantity},
};

let gold = Commodity::builder()
    .instrument_id(InstrumentId::from("GOLD.COMEX"))
    .raw_symbol(Symbol::from("GOLD"))
    .asset_class(AssetClass::Commodity)
    .quote_currency(Currency::from("USD"))
    .price_precision(2)
    .size_precision(0)
    .price_increment(Price::from("0.01"))
    .size_increment(Quantity::from("1"))
    .lot_size(Quantity::from("1"))
    .ts_event(UnixNanos::default())
    .ts_init(UnixNanos::default())
    .build()
    .unwrap();
```

```python tab="Python"
from nautilus_trader.model import AssetClass
from nautilus_trader.model import Commodity
from nautilus_trader.model import Currency
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import Symbol

gold = Commodity(
    instrument_id=InstrumentId.from_str("GOLD.COMEX"),
    raw_symbol=Symbol("GOLD"),
    asset_class=AssetClass.COMMODITY,
    quote_currency=Currency.from_str("USD"),
    price_precision=2,
    price_increment=Price.from_str("0.01"),
    size_precision=0,
    size_increment=Quantity.from_int(1),
    ts_event=0,
    ts_init=0,
    lot_size=Quantity.from_int(1),
)
```

## Adapters

Representative adapters that create or consume `Commodity` instruments include:

- [Interactive Brokers](../../integrations/ib.md) for spot commodity and metal contracts.

## Related guides

- [Futures Contract](futures_contract.md) covers dated futures on commodity underlyings.
- [Data](../data.md) explains market data that references instruments.
