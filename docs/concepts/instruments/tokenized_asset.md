# Tokenized Asset

`TokenizedAsset` represents a spot-like token that tracks another asset on a crypto venue.
Use it for tokenized equities, tokenized funds, or similar instruments where the trading
venue exposes a token but the economic reference is an external asset.

Examples include tokenized stock or ETF symbols on crypto venues.

## Fields

<Tabs items={["Rust", "Python"]}>
<Tab value="Rust">

| Field             | Type               | Required/default | Notes                                    |
|-------------------|--------------------|------------------|------------------------------------------|
| `instrument_id`   | `InstrumentId`     | Required         | Stored as `id` in Rust.                  |
| `raw_symbol`      | `Symbol`           | Required         | Native venue symbol.                     |
| `asset_class`     | `AssetClass`       | Required         | Economic asset classification.           |
| `base_currency`   | `Currency`         | Required         | Tokenized asset or base token.           |
| `quote_currency`  | `Currency`         | Required         | Currency used to price the token.        |
| `price_precision` | `u8`               | Required         | Decimal places allowed for prices.       |
| `size_precision`  | `u8`               | Required         | Decimal places allowed for order sizes.  |
| `price_increment` | `Price`            | Required         | Smallest valid price step.               |
| `size_increment`  | `Quantity`         | Required         | Smallest valid size step.                |
| `ts_event`        | `UnixNanos`        | Required         | Event timestamp in nanoseconds.          |
| `ts_init`         | `UnixNanos`        | Required         | Initialization timestamp in nanoseconds. |
| `isin`            | `Option<Ustr>`     | `None`           | International Securities ID when known.  |
| `multiplier`      | `Quantity`         | `1`              | Contract multiplier.                     |
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
| `asset_class`     | `AssetClass`       | Required         | Economic asset classification.           |
| `base_currency`   | `Currency`         | Required         | Tokenized asset or base token.           |
| `quote_currency`  | `Currency`         | Required         | Currency used to price the token.        |
| `price_precision` | `int`              | Required         | Decimal places allowed for prices.       |
| `size_precision`  | `int`              | Required         | Decimal places allowed for order sizes.  |
| `price_increment` | `Price`            | Required         | Smallest valid price step.               |
| `size_increment`  | `Quantity`         | Required         | Smallest valid size step.                |
| `ts_event`        | `int`              | Required         | Event timestamp in nanoseconds.          |
| `ts_init`         | `int`              | Required         | Initialization timestamp in nanoseconds. |
| `isin`            | `str \| None`      | `None`           | International Securities ID when known.  |
| `multiplier`      | `Quantity`         | `1`              | Contract multiplier.                     |
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

- `TokenizedAsset` has instrument class `Spot`.
- It is never inverse, and its cost currency is the quote currency.
- It can carry an `isin` when the token references a listed security.
- It has no activation timestamp, expiry, strike, or option kind.

## Example

```rust tab="Rust"
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::AssetClass,
    identifiers::{InstrumentId, Symbol},
    instruments::TokenizedAsset,
    types::{Currency, Price, Quantity},
};
use rust_decimal_macros::dec;

let aaplx = TokenizedAsset::builder()
    .instrument_id(InstrumentId::from("AAPLx/USD.KRAKEN"))
    .raw_symbol(Symbol::from("AAPLxUSD"))
    .asset_class(AssetClass::Equity)
    .base_currency(Currency::get_or_create_crypto("AAPLx"))
    .quote_currency(Currency::from("USD"))
    .price_precision(2)
    .size_precision(4)
    .price_increment(Price::from("0.01"))
    .size_increment(Quantity::from("0.0001"))
    .min_quantity(Quantity::from("0.0001"))
    .maker_fee(dec!(-0.0002))
    .taker_fee(dec!(0.001))
    .ts_event(UnixNanos::default())
    .ts_init(UnixNanos::default())
    .build()
    .unwrap();
```

```python tab="Python"
from decimal import Decimal

from nautilus_trader.model import AssetClass
from nautilus_trader.model import Currency
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import Symbol
from nautilus_trader.model import TokenizedAsset

aaplx = TokenizedAsset(
    instrument_id=InstrumentId.from_str("AAPLx/USD.KRAKEN"),
    raw_symbol=Symbol("AAPLxUSD"),
    asset_class=AssetClass.EQUITY,
    base_currency=Currency.from_str("AAPLx"),
    quote_currency=Currency.from_str("USD"),
    price_precision=2,
    size_precision=4,
    price_increment=Price.from_str("0.01"),
    size_increment=Quantity.from_str("0.0001"),
    ts_event=0,
    ts_init=0,
    min_quantity=Quantity.from_str("0.0001"),
    maker_fee=Decimal("-0.0002"),
    taker_fee=Decimal("0.001"),
)
```

## Adapters

Representative adapters that create or consume `TokenizedAsset` instruments include:

- [Kraken](../../integrations/kraken.md) for tokenized assets where the venue exposes them.

## Related guides

- [Currency Pair](currency_pair.md) covers ordinary crypto spot pairs.
- [Equity](equity.md) covers listed cash equities.
