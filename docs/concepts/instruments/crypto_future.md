# Crypto Future

`CryptoFuture` represents a dated crypto futures contract. It tracks a crypto
underlying, quotes in a quote currency, settles in a settlement currency, and expires at
a fixed timestamp.

Examples include dated BTC or ETH futures on crypto derivatives venues.

## Fields

| Field                 | Rust type          | Python type       | Required/default | Notes                                   |
|-----------------------|--------------------|-------------------|------------------|-----------------------------------------|
| `instrument_id`       | `InstrumentId`     | `InstrumentId`    | Required         | Stored as `id` in Rust.                 |
| `raw_symbol`          | `Symbol`           | `Symbol`          | Required         | Native venue symbol.                    |
| `underlying`          | `Currency`         | `Currency`        | Required         | Crypto asset the contract tracks.       |
| `quote_currency`      | `Currency`         | `Currency`        | Required         | Currency used to quote the price.       |
| `settlement_currency` | `Currency`         | `Currency`        | Required         | Currency used to settle PnL and fees.   |
| `is_inverse`          | `bool`             | `bool`            | Required         | True when sizing/costing is inverse.    |
| `activation_ns`       | `UnixNanos`        | `int`             | Required         | Contract activation timestamp.          |
| `expiration_ns`       | `UnixNanos`        | `int`             | Required         | Contract expiration timestamp.          |
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

- `CryptoFuture` has asset class `Cryptocurrency` and instrument class `Future`.
- Linear contracts typically set `is_inverse=False` and settle in the quote currency.
- Inverse contracts set `is_inverse=True` and typically settle in the underlying currency.
- Quanto contracts settle in a third currency that differs from both underlying and quote.
- Use `CryptoPerpetual` for crypto derivatives with no expiration.

## Example

```rust tab="Rust"
use chrono::{TimeZone, Utc};
use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::{InstrumentId, Symbol},
    instruments::CryptoFuture,
    types::{Currency, Money, Price, Quantity},
};

let activation = Utc.with_ymd_and_hms(2024, 1, 8, 0, 0, 0).unwrap();
let expiration = Utc.with_ymd_and_hms(2024, 3, 29, 0, 0, 0).unwrap();

let btcusdt_future = CryptoFuture::new(
    InstrumentId::from("BTCUSDT-240329.BINANCE"),
    Symbol::from("BTCUSDT-240329"),
    Currency::from("BTC"),
    Currency::from("USDT"),
    Currency::from("USDT"),
    false,
    UnixNanos::from(activation.timestamp_nanos_opt().unwrap() as u64),
    UnixNanos::from(expiration.timestamp_nanos_opt().unwrap() as u64),
    2,
    6,
    Price::from("0.01"),
    Quantity::from("0.000001"),
    None,
    None,
    Some(Quantity::from("9000.0")),
    Some(Quantity::from("0.000001")),
    None,
    Some(Money::from("10.00 USDT")),
    Some(Price::from("1000000.00")),
    Some(Price::from("0.01")),
    None,
    None,
    None,
    None,
    None,
    UnixNanos::default(),
    UnixNanos::default(),
);
```

```python tab="Python"
import pandas as pd

from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import CryptoFuture
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity

btcusdt_future = CryptoFuture(
    instrument_id=InstrumentId.from_str("BTCUSDT-240329.BINANCE"),
    raw_symbol=Symbol("BTCUSDT-240329"),
    underlying=BTC,
    quote_currency=USDT,
    settlement_currency=USDT,
    is_inverse=False,
    activation_ns=pd.Timestamp("2024-01-08", tz="UTC").value,
    expiration_ns=pd.Timestamp("2024-03-29", tz="UTC").value,
    price_precision=2,
    size_precision=6,
    price_increment=Price.from_str("0.01"),
    size_increment=Quantity.from_str("0.000001"),
    max_quantity=Quantity.from_str("9000"),
    min_quantity=Quantity.from_str("0.000001"),
    min_notional=Money(10.00, USDT),
    max_price=Price.from_str("1000000.00"),
    min_price=Price.from_str("0.01"),
    ts_event=0,
    ts_init=0,
)
```

## Adapters

Representative adapters that create or consume `CryptoFuture` instruments include:

- [BitMEX](../../integrations/bitmex.md) for inverse and linear dated futures.
- [Bybit](../../integrations/bybit.md) for crypto futures markets.
- [Deribit](../../integrations/deribit.md) for dated crypto futures.
- [OKX](../../integrations/okx.md) for dated crypto futures.
- [Tardis](../../integrations/tardis.md) for crypto futures metadata.

## Related guides

- [Crypto Perpetual](crypto_perpetual.md) covers perpetual crypto futures.
- [Futures Contract](futures_contract.md) covers non-crypto futures contracts.
