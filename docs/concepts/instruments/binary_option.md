# Binary Option

`BinaryOption` represents a binary outcome instrument that settles to a fixed payoff
based on whether a condition is true. It can model prediction markets, binary options,
or venue-specific yes/no contracts.

Examples include prediction market outcomes and binary event contracts.

## Fields

| Field              | Rust type          | Python type       | Required/default | Notes                                   |
|--------------------|--------------------|-------------------|------------------|-----------------------------------------|
| `instrument_id`    | `InstrumentId`     | `InstrumentId`    | Required         | Stored as `id` in Rust.                 |
| `raw_symbol`       | `Symbol`           | `Symbol`          | Required         | Native venue symbol.                    |
| `asset_class`      | `AssetClass`       | `AssetClass`      | Required         | Asset class of the outcome market.      |
| `currency`         | `Currency`         | `Currency`        | Required         | Quote and settlement currency.          |
| `activation_ns`    | `UnixNanos`        | `int`             | Required         | Contract activation timestamp.          |
| `expiration_ns`    | `UnixNanos`        | `int`             | Required         | Contract expiration timestamp.          |
| `price_precision`  | `u8`               | `int`             | Required         | Decimal places allowed for prices.      |
| `size_precision`   | `u8`               | `int`             | Required         | Decimal places allowed for order sizes. |
| `price_increment`  | `Price`            | `Price`           | Required         | Smallest valid price step.              |
| `size_increment`   | `Quantity`         | `Quantity`        | Required         | Smallest valid size step.               |
| `outcome`          | `Option<Ustr>`     | `str \| None`      | `None`           | Outcome label when the venue provides it. |
| `description`      | `Option<Ustr>`     | `str \| None`      | `None`           | Human‑readable market description.      |
| `max_quantity`     | `Option<Quantity>` | `Quantity \| None` | `None`           | Maximum order quantity.                 |
| `min_quantity`     | `Option<Quantity>` | `Quantity \| None` | `None`           | Minimum order quantity.                 |
| `max_notional`     | `Option<Money>`    | N/A               | Rust only        | Maximum order notional value.           |
| `min_notional`     | `Option<Money>`    | N/A               | Rust only        | Minimum order notional value.           |
| `max_price`        | `Option<Price>`    | N/A               | Rust only        | Maximum valid quote or order price.     |
| `min_price`        | `Option<Price>`    | N/A               | Rust only        | Minimum valid quote or order price.     |
| `margin_init`      | `Option<Decimal>`  | N/A               | Rust only        | Initial margin rate.                    |
| `margin_maint`     | `Option<Decimal>`  | N/A               | Rust only        | Maintenance margin rate.                |
| `maker_fee`        | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Maker fee rate. Negative values rebate. |
| `taker_fee`        | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Taker fee rate. Negative values rebate. |
| `tick_scheme_name` | N/A                | `str \| None`      | `None`           | Registered variable tick scheme name.   |
| `info`             | `Option<Params>`   | `dict \| None`     | `None`           | Adapter metadata.                       |
| `ts_event`         | `UnixNanos`        | `int`             | Required         | Event timestamp in nanoseconds.         |
| `ts_init`          | `UnixNanos`        | `int`             | Required         | Initialization timestamp in nanoseconds. |

*Note: Python constructors use `instrument_id`; Rust stores the same value as `id`.*

## Behavior

- `BinaryOption` has instrument class `BinaryOption`.
- It is never inverse and uses a multiplier and lot size of one.
- Many venues quote binary outcomes between zero and one, but the venue defines the
  allowed price range and tick size.
- `outcome` and `description` provide human-readable context for the contract.

## Example

```rust tab="Rust"
use chrono::{TimeZone, Utc};
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::AssetClass,
    identifiers::{InstrumentId, Symbol, Venue},
    instruments::BinaryOption,
    types::{Currency, Price, Quantity},
};
use rust_decimal_macros::dec;
use ustr::Ustr;

let raw_symbol = Symbol::from(
    "0x12a0cb60174abc437bf1178367c72d11f069e1a3add20b148fb0ab4279b772b2-92544998123698303655208967887569360731013655782348975589292031774495159624905",
);
let expiration = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();

let yes_outcome = BinaryOption::new(
    InstrumentId::new(raw_symbol, Venue::from("POLYMARKET")),
    raw_symbol,
    AssetClass::Alternative,
    Currency::from("USDC"),
    UnixNanos::default(),
    UnixNanos::from(expiration.timestamp_nanos_opt().unwrap() as u64),
    3,
    2,
    Price::from("0.001"),
    Quantity::from("0.01"),
    Some(Ustr::from("Yes")),
    Some(Ustr::from("Will the outcome of this market be 'Yes'?")),
    None,
    Some(Quantity::from("5")),
    None,
    None,
    None,
    None,
    None,
    None,
    Some(dec!(0)),
    Some(dec!(0)),
    None,
    UnixNanos::default(),
    UnixNanos::default(),
);
```

```python tab="Python"
from decimal import Decimal

import pandas as pd

from nautilus_trader.model.currencies import USDC
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import BinaryOption
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity

raw_symbol = Symbol(
    "0x12a0cb60174abc437bf1178367c72d11f069e1a3add20b148fb0ab4279b772b2-92544998123698303655208967887569360731013655782348975589292031774495159624905",
)
price_increment = Price.from_str("0.001")
size_increment = Quantity.from_str("0.01")

yes_outcome = BinaryOption(
    instrument_id=InstrumentId(symbol=raw_symbol, venue=Venue("POLYMARKET")),
    raw_symbol=raw_symbol,
    asset_class=AssetClass.ALTERNATIVE,
    currency=USDC,
    price_precision=price_increment.precision,
    size_precision=size_increment.precision,
    price_increment=price_increment,
    size_increment=size_increment,
    activation_ns=0,
    expiration_ns=pd.Timestamp("2024-01-01", tz="UTC").value,
    min_quantity=Quantity.from_int(5),
    maker_fee=Decimal(0),
    taker_fee=Decimal(0),
    outcome="Yes",
    description="Will the outcome of this market be 'Yes'?",
    ts_event=0,
    ts_init=0,
)
```

## Adapters

Representative adapters that create or consume `BinaryOption` instruments include:

- [Hyperliquid](../../integrations/hyperliquid.md) for binary and prediction-style markets.
- [OKX](../../integrations/okx.md) for venue-defined binary outcome products.
- [Polymarket](../../integrations/polymarket.md) for prediction market outcomes.

## Related guides

- [Order Book](../order_book.md) covers binary market order book behavior.
- [Data](../data.md) explains market data that references instruments.
