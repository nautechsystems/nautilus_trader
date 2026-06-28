# Crypto Future

`CryptoFuture` represents a dated crypto futures contract. It tracks a crypto
underlying, quotes in a quote currency, settles in a settlement currency, and expires at
a fixed timestamp.

Examples include dated BTC or ETH futures on crypto derivatives venues.

## Fields

<Tabs items={["Rust", "Python"]}>
<Tab value="Rust">

| Field                 | Type               | Required/default | Notes                                    |
|-----------------------|--------------------|------------------|------------------------------------------|
| `instrument_id`       | `InstrumentId`     | Required         | Stored as `id` in Rust.                  |
| `raw_symbol`          | `Symbol`           | Required         | Native venue symbol.                     |
| `underlying`          | `Currency`         | Required         | Crypto asset the contract tracks.        |
| `quote_currency`      | `Currency`         | Required         | Currency used to quote the price.        |
| `settlement_currency` | `Currency`         | Required         | Currency used to settle PnL and fees.    |
| `is_inverse`          | `bool`             | Required         | True when sizing/costing is inverse.     |
| `activation_ns`       | `UnixNanos`        | Required         | Contract activation timestamp.           |
| `expiration_ns`       | `UnixNanos`        | Required         | Contract expiration timestamp.           |
| `price_precision`     | `u8`               | Required         | Decimal places allowed for prices.       |
| `size_precision`      | `u8`               | Required         | Decimal places allowed for order sizes.  |
| `price_increment`     | `Price`            | Required         | Smallest valid price step.               |
| `size_increment`      | `Quantity`         | Required         | Smallest valid size step.                |
| `multiplier`          | `Quantity`         | `1`              | Contract multiplier.                     |
| `lot_size`            | `Quantity`         | `1`              | Rounded lot or board size.               |
| `max_quantity`        | `Option<Quantity>` | `None`           | Maximum order quantity.                  |
| `min_quantity`        | `Option<Quantity>` | `None`           | Minimum order quantity.                  |
| `max_notional`        | `Option<Money>`    | `None`           | Maximum order notional value.            |
| `min_notional`        | `Option<Money>`    | `None`           | Minimum order notional value.            |
| `max_price`           | `Option<Price>`    | `None`           | Maximum valid quote or order price.      |
| `min_price`           | `Option<Price>`    | `None`           | Minimum valid quote or order price.      |
| `margin_init`         | `Option<Decimal>`  | `0`              | Initial margin rate.                     |
| `margin_maint`        | `Option<Decimal>`  | `0`              | Maintenance margin rate.                 |
| `maker_fee`           | `Option<Decimal>`  | `0`              | Maker fee rate. Negative values rebate.  |
| `taker_fee`           | `Option<Decimal>`  | `0`              | Taker fee rate. Negative values rebate.  |
| `tick_scheme`         | `Option<Ustr>`     | `None`           | Registered variable tick scheme name.    |
| `info`                | `Option<Params>`   | `None`           | Adapter metadata.                        |
| `ts_event`            | `UnixNanos`        | Required         | Event timestamp in nanoseconds.          |
| `ts_init`             | `UnixNanos`        | Required         | Initialization timestamp in nanoseconds. |

</Tab>
<Tab value="Python">

| Field                 | Type               | Required/default | Notes                                    |
|-----------------------|--------------------|------------------|------------------------------------------|
| `instrument_id`       | `InstrumentId`     | Required         |                                          |
| `raw_symbol`          | `Symbol`           | Required         | Native venue symbol.                     |
| `underlying`          | `Currency`         | Required         | Crypto asset the contract tracks.        |
| `quote_currency`      | `Currency`         | Required         | Currency used to quote the price.        |
| `settlement_currency` | `Currency`         | Required         | Currency used to settle PnL and fees.    |
| `is_inverse`          | `bool`             | Required         | True when sizing/costing is inverse.     |
| `activation_ns`       | `int`              | Required         | Contract activation timestamp.           |
| `expiration_ns`       | `int`              | Required         | Contract expiration timestamp.           |
| `price_precision`     | `int`              | Required         | Decimal places allowed for prices.       |
| `size_precision`      | `int`              | Required         | Decimal places allowed for order sizes.  |
| `price_increment`     | `Price`            | Required         | Smallest valid price step.               |
| `size_increment`      | `Quantity`         | Required         | Smallest valid size step.                |
| `multiplier`          | `Quantity`         | `1`              | Contract multiplier.                     |
| `lot_size`            | `Quantity`         | `1`              | Rounded lot or board size.               |
| `max_quantity`        | `Quantity \| None` | `None`           | Maximum order quantity.                  |
| `min_quantity`        | `Quantity \| None` | `None`           | Minimum order quantity.                  |
| `max_notional`        | `Money \| None`    | `None`           | Maximum order notional value.            |
| `min_notional`        | `Money \| None`    | `None`           | Minimum order notional value.            |
| `max_price`           | `Price \| None`    | `None`           | Maximum valid quote or order price.      |
| `min_price`           | `Price \| None`    | `None`           | Minimum valid quote or order price.      |
| `margin_init`         | `Decimal \| None`  | `0`              | Initial margin rate.                     |
| `margin_maint`        | `Decimal \| None`  | `0`              | Maintenance margin rate.                 |
| `maker_fee`           | `Decimal \| None`  | `0`              | Maker fee rate. Negative values rebate.  |
| `taker_fee`           | `Decimal \| None`  | `0`              | Taker fee rate. Negative values rebate.  |
| `tick_scheme`         | `str \| None`      | `None`           | Registered variable tick scheme name.    |
| `info`                | `dict \| None`     | `None`           | Adapter metadata.                        |
| `ts_event`            | `int`              | Required         | Event timestamp in nanoseconds.          |
| `ts_init`             | `int`              | Required         | Initialization timestamp in nanoseconds. |

</Tab>
</Tabs>

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

let btcusdt_future = CryptoFuture::builder()
    .instrument_id(InstrumentId::from("BTCUSDT-240329.BINANCE"))
    .raw_symbol(Symbol::from("BTCUSDT-240329"))
    .underlying(Currency::from("BTC"))
    .quote_currency(Currency::from("USDT"))
    .settlement_currency(Currency::from("USDT"))
    .is_inverse(false)
    .activation_ns(UnixNanos::from(activation.timestamp_nanos_opt().unwrap() as u64))
    .expiration_ns(UnixNanos::from(expiration.timestamp_nanos_opt().unwrap() as u64))
    .price_precision(2)
    .size_precision(6)
    .price_increment(Price::from("0.01"))
    .size_increment(Quantity::from("0.000001"))
    .max_quantity(Quantity::from("9000.0"))
    .min_quantity(Quantity::from("0.000001"))
    .min_notional(Money::from("10.00 USDT"))
    .max_price(Price::from("1000000.00"))
    .min_price(Price::from("0.01"))
    .ts_event(UnixNanos::default())
    .ts_init(UnixNanos::default())
    .build()
    .unwrap();
```

```python tab="Python"
import pandas as pd

from nautilus_trader.model import CryptoFuture
from nautilus_trader.model import Currency
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Money
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import Symbol

BTC = Currency.from_str("BTC")
USDT = Currency.from_str("USDT")

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
