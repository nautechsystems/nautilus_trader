# Perpetual Contract

`PerpetualContract` represents a generic perpetual futures contract across asset classes.
Use it when a venue exposes a perpetual swap that is not specifically modeled as
`CryptoPerpetual`.

Examples include non-crypto perpetual contracts and venue-specific synthetic swaps.

## Fields

| Field                 | Rust type          | Python type       | Required/default | Notes                                   |
|-----------------------|--------------------|-------------------|------------------|-----------------------------------------|
| `instrument_id`       | `InstrumentId`     | `InstrumentId`    | Required         | Stored as `id` in Rust.                 |
| `raw_symbol`          | `Symbol`           | `Symbol`          | Required         | Native venue symbol.                    |
| `underlying`          | `Ustr`             | `str`             | Required         | Underlying asset or reference market.   |
| `asset_class`         | `AssetClass`       | `AssetClass`      | Required         | Asset class of the underlying.          |
| `base_currency`       | `Option<Currency>` | `Currency \| None` | `None`           | Base currency, required for inverse.    |
| `quote_currency`      | `Currency`         | `Currency`        | Required         | Currency used to quote the price.       |
| `settlement_currency` | `Currency`         | `Currency`        | Required         | Currency used to settle PnL and fees.   |
| `is_inverse`          | `bool`             | `bool`            | Required         | True when sizing/costing is inverse.    |
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

- `PerpetualContract` has instrument class `Swap`.
- It has no activation timestamp or expiration timestamp.
- Inverse contracts require a base currency.
- Linear contracts typically settle in the quote currency.
- Use `CryptoPerpetual` for crypto perpetuals where the base asset is a currency.

## Example

```rust tab="Rust"
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::AssetClass,
    identifiers::{InstrumentId, Symbol},
    instruments::PerpetualContract,
    types::{Currency, Price, Quantity},
};
use rust_decimal_macros::dec;
use ustr::Ustr;

let eurusd_perp = PerpetualContract::new(
    InstrumentId::from("EURUSD-PERP.AX"),
    Symbol::from("EURUSD-PERP"),
    Ustr::from("EURUSD"),
    AssetClass::FX,
    Some(Currency::from("EUR")),
    Currency::from("USD"),
    Currency::from("USD"),
    false,
    5,
    0,
    Price::from("0.00001"),
    Quantity::from("1"),
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    None,
    Some(dec!(0.03)),
    Some(dec!(0.03)),
    Some(dec!(0.00002)),
    Some(dec!(0.00002)),
    None,
    UnixNanos::default(),
    UnixNanos::default(),
);
```

```python tab="Python"
from decimal import Decimal

from nautilus_trader.model.currencies import EUR
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import PerpetualContract
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity

eurusd_perp = PerpetualContract(
    instrument_id=InstrumentId.from_str("EURUSD-PERP.AX"),
    raw_symbol=Symbol("EURUSD-PERP"),
    underlying="EURUSD",
    asset_class=AssetClass.FX,
    base_currency=EUR,
    quote_currency=USD,
    settlement_currency=USD,
    is_inverse=False,
    price_precision=5,
    size_precision=0,
    price_increment=Price.from_str("0.00001"),
    size_increment=Quantity.from_int(1),
    margin_init=Decimal("0.03"),
    margin_maint=Decimal("0.03"),
    maker_fee=Decimal("0.00002"),
    taker_fee=Decimal("0.00002"),
    ts_event=0,
    ts_init=0,
)
```

## Adapters

Representative adapters that create or consume `PerpetualContract` instruments include:

- [Architect AX](../../integrations/architect_ax.md) for venue-defined perpetual contracts.

## Related guides

- [Crypto Perpetual](crypto_perpetual.md) covers crypto perpetual futures.
- [Data](../data.md) covers mark prices, index prices, and funding rate updates.
