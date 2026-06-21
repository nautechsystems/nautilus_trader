# Commodity

`Commodity` represents a spot commodity market such as gold, silver, oil, or another
physical asset quoted in a currency. It models a spot market, not a dated futures
contract.

Examples include `XAUUSD.IDEALPRO` and venue-specific commodity cash symbols.

## Fields

| Field              | Rust type          | Python type       | Required/default | Notes                                      |
|--------------------|--------------------|-------------------|------------------|--------------------------------------------|
| `instrument_id`    | `InstrumentId`     | `InstrumentId`    | Required         | Stored as `id` in Rust.                    |
| `raw_symbol`       | `Symbol`           | `Symbol`          | Required         | Native venue symbol.                       |
| `asset_class`      | `AssetClass`       | `AssetClass`      | Required         | Commodity asset classification.            |
| `quote_currency`   | `Currency`         | `Currency`        | Required         | Currency used to price the commodity.      |
| `price_precision`  | `u8`               | `int`             | Required         | Decimal places allowed for prices.         |
| `size_precision`   | `u8`               | `int`             | Required         | Decimal places allowed for order sizes.    |
| `price_increment`  | `Price`            | `Price`           | Required         | Smallest valid price step.                 |
| `size_increment`   | `Quantity`         | `Quantity`        | Required         | Smallest valid size step.                  |
| `ts_event`         | `UnixNanos`        | `int`             | Required         | Event timestamp in nanoseconds.            |
| `ts_init`          | `UnixNanos`        | `int`             | Required         | Initialization timestamp in nanoseconds.   |
| `base_currency`    | N/A                | `Currency \| None` | `None`           | Python‑only base asset currency, if known. |
| `lot_size`         | `Option<Quantity>` | `Quantity \| None` | `None`           | Rounded lot or board size.                 |
| `max_quantity`     | `Option<Quantity>` | `Quantity \| None` | `None`           | Maximum order quantity.                    |
| `min_quantity`     | `Option<Quantity>` | `Quantity \| None` | `None`           | Minimum order quantity.                    |
| `max_notional`     | `Option<Money>`    | `Money \| None`    | `None`           | Maximum order notional value.              |
| `min_notional`     | `Option<Money>`    | `Money \| None`    | `None`           | Minimum order notional value.              |
| `max_price`        | `Option<Price>`    | `Price \| None`    | `None`           | Maximum valid quote or order price.        |
| `min_price`        | `Option<Price>`    | `Price \| None`    | `None`           | Minimum valid quote or order price.        |
| `margin_init`      | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Initial margin rate.                       |
| `margin_maint`     | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Maintenance margin rate.                   |
| `maker_fee`        | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Maker fee rate. Negative values rebate.    |
| `taker_fee`        | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Taker fee rate. Negative values rebate.    |
| `tick_scheme_name` | N/A                | `str \| None`      | `None`           | Registered variable tick scheme name.      |
| `info`             | `Option<Params>`   | `dict \| None`     | `None`           | Adapter metadata.                          |

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
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import Commodity
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity

gold = Commodity(
    instrument_id=InstrumentId.from_str("GOLD.COMEX"),
    raw_symbol=Symbol("GOLD"),
    asset_class=AssetClass.COMMODITY,
    quote_currency=USD,
    price_precision=2,
    price_increment=Price.from_str("0.01"),
    size_precision=0,
    size_increment=Quantity.from_int(1),
    lot_size=Quantity.from_int(1),
    ts_event=0,
    ts_init=0,
)
```

## Adapters

Representative adapters that create or consume `Commodity` instruments include:

- [Interactive Brokers](../../integrations/ib.md) for spot commodity and metal contracts.

## Related guides

- [Futures Contract](futures_contract.md) covers dated futures on commodity underlyings.
- [Data](../data.md) explains market data that references instruments.
