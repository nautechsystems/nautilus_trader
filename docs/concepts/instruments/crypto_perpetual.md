# Crypto Perpetual

`CryptoPerpetual` represents a crypto perpetual futures contract, also known as a
perpetual swap. It has no expiry, tracks a crypto base asset, and settles in a crypto,
stablecoin, or other venue-defined settlement currency.

Examples include `ETHUSDT-PERP.BINANCE`, `XBTUSD.BITMEX`, and `BTC-USD-SWAP.OKX`.

## Fields

<Tabs items={["Rust", "Python"]}>
<Tab value="Rust">

| Field                 | Type               | Required/default | Notes                                    |
|-----------------------|--------------------|------------------|------------------------------------------|
| `instrument_id`       | `InstrumentId`     | Required         | Stored as `id` in Rust.                  |
| `raw_symbol`          | `Symbol`           | Required         | Native venue symbol.                     |
| `base_currency`       | `Currency`         | Required         | Base crypto asset.                       |
| `quote_currency`      | `Currency`         | Required         | Price quote currency.                    |
| `settlement_currency` | `Currency`         | Required         | Currency used to settle PnL and fees.    |
| `is_inverse`          | `bool`             | Required         | True when sizing/costing is inverse.     |
| `price_precision`     | `u8`               | Required         | Decimal places allowed for prices.       |
| `size_precision`      | `u8`               | Required         | Decimal places allowed for order sizes.  |
| `price_increment`     | `Price`            | Required         | Smallest valid price step.               |
| `size_increment`      | `Quantity`         | Required         | Smallest valid size step.                |
| `ts_event`            | `UnixNanos`        | Required         | Event timestamp in nanoseconds.          |
| `ts_init`             | `UnixNanos`        | Required         | Initialization timestamp in nanoseconds. |
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

</Tab>
<Tab value="Python">

| Field                 | Type               | Required/default | Notes                                    |
|-----------------------|--------------------|------------------|------------------------------------------|
| `instrument_id`       | `InstrumentId`     | Required         |                                          |
| `raw_symbol`          | `Symbol`           | Required         | Native venue symbol.                     |
| `base_currency`       | `Currency`         | Required         | Base crypto asset.                       |
| `quote_currency`      | `Currency`         | Required         | Price quote currency.                    |
| `settlement_currency` | `Currency`         | Required         | Currency used to settle PnL and fees.    |
| `is_inverse`          | `bool`             | Required         | True when sizing/costing is inverse.     |
| `price_precision`     | `int`              | Required         | Decimal places allowed for prices.       |
| `size_precision`      | `int`              | Required         | Decimal places allowed for order sizes.  |
| `price_increment`     | `Price`            | Required         | Smallest valid price step.               |
| `size_increment`      | `Quantity`         | Required         | Smallest valid size step.                |
| `ts_event`            | `int`              | Required         | Event timestamp in nanoseconds.          |
| `ts_init`             | `int`              | Required         | Initialization timestamp in nanoseconds. |
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

</Tab>
</Tabs>

*Note: Python constructors use `instrument_id`; Rust stores the same value as `id`.*

## Behavior

- `CryptoPerpetual` has asset class `Cryptocurrency` and instrument class `Swap`.
- It has no activation or expiration timestamp.
- Linear contracts typically set `is_inverse=False` and settle in the quote currency.
- Inverse contracts set `is_inverse=True` and typically settle in the base currency.
- Quanto contracts settle in a third currency that differs from both base and quote.
- The cost currency is base for inverse contracts, settlement for quanto contracts, and
  quote otherwise.

:::note
Funding payments are not fields on the instrument. They arrive as data, such as
`FundingRateUpdate`, and reference the instrument ID.
:::

## Example

```rust tab="Rust"
use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::{InstrumentId, Symbol},
    instruments::{CryptoPerpetual, InstrumentAny},
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal_macros::dec;

let ethusdt_perp = CryptoPerpetual::builder()
    .instrument_id(InstrumentId::from("ETHUSDT-PERP.BINANCE"))
    .raw_symbol(Symbol::from("ETHUSDT"))
    .base_currency(Currency::from("ETH"))
    .quote_currency(Currency::from("USDT"))
    .settlement_currency(Currency::from("USDT"))
    .is_inverse(false)
    .price_precision(2)
    .size_precision(3)
    .price_increment(Price::from("0.01"))
    .size_increment(Quantity::from("0.001"))
    .max_quantity(Quantity::from("10000.000"))
    .min_quantity(Quantity::from("0.001"))
    .min_notional(Money::from("10.00 USDT"))
    .max_price(Price::from("15000.00"))
    .min_price(Price::from("1.00"))
    .margin_init(dec!(1.0))
    .margin_maint(dec!(0.35))
    .maker_fee(dec!(0.0002))
    .taker_fee(dec!(0.0004))
    .ts_event(UnixNanos::default())
    .ts_init(UnixNanos::default())
    .build()
    .unwrap();

let instrument = InstrumentAny::CryptoPerpetual(ethusdt_perp);
```

```python tab="Python"
from decimal import Decimal

from nautilus_trader.model import CryptoPerpetual
from nautilus_trader.model import Currency
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Money
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import Symbol

ETH = Currency.from_str("ETH")
USDT = Currency.from_str("USDT")

ethusdt_perp = CryptoPerpetual(
    instrument_id=InstrumentId.from_str("ETHUSDT-PERP.BINANCE"),
    raw_symbol=Symbol("ETHUSDT"),
    base_currency=ETH,
    quote_currency=USDT,
    settlement_currency=USDT,
    is_inverse=False,
    price_precision=2,
    size_precision=3,
    price_increment=Price.from_str("0.01"),
    size_increment=Quantity.from_str("0.001"),
    ts_event=0,
    ts_init=0,
    max_quantity=Quantity.from_str("10000.000"),
    min_quantity=Quantity.from_str("0.001"),
    min_notional=Money(10.00, USDT),
    max_price=Price.from_str("15000.00"),
    min_price=Price.from_str("1.00"),
    margin_init=Decimal("1.0"),
    margin_maint=Decimal("0.35"),
    maker_fee=Decimal("0.0002"),
    taker_fee=Decimal("0.0004"),
)
```

## Adapters

Representative adapters that create or consume `CryptoPerpetual` instruments include:

- [Binance](../../integrations/binance.md) for USD-M and COIN-M perpetual futures.
- [BitMEX](../../integrations/bitmex.md) for inverse and linear perpetual contracts.
- [Bybit](../../integrations/bybit.md) for linear and inverse perpetual products.
- [dYdX](../../integrations/dydx.md) for perpetual markets.
- [Hyperliquid](../../integrations/hyperliquid.md) for perpetual markets.
- [Kraken](../../integrations/kraken.md) for futures venue perpetual markets.
- [OKX](../../integrations/okx.md) for swap markets.
- [Tardis](../../integrations/tardis.md) for crypto perpetual metadata.

## Related guides

- [Data](../data.md) covers mark prices, index prices, and funding rate updates.
- [Options](../options.md) covers option-specific instrument types.
- [Execution](../execution.md) explains precision and notional checks before orders reach a venue.
