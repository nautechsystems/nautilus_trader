# Crypto Perpetual

`CryptoPerpetual` represents a crypto perpetual futures contract, also known as a
perpetual swap. It has no expiry, tracks a crypto base asset, and settles in a crypto,
stablecoin, or other venue-defined settlement currency.

Examples include `ETHUSDT-PERP.BINANCE`, `XBTUSD.BITMEX`, and `BTC-USD-SWAP.OKX`.

## Fields

| Field                 | Rust type          | Python type            | Required/default     | Notes                                      |
|-----------------------|--------------------|------------------------|----------------------|--------------------------------------------|
| `instrument_id`       | `InstrumentId`     | `InstrumentId`         | Required             | Stored as `id` in Rust.                    |
| `raw_symbol`          | `Symbol`           | `Symbol`               | Required             | Native venue symbol.                       |
| `base_currency`       | `Currency`         | `Currency`             | Required             | Base crypto asset.                         |
| `quote_currency`      | `Currency`         | `Currency`             | Required             | Price quote currency.                      |
| `settlement_currency` | `Currency`         | `Currency`             | Required             | Currency used to settle PnL and fees.      |
| `is_inverse`          | `bool`             | `bool`                 | Required             | True when sizing/costing is inverse.       |
| `price_precision`     | `u8`               | `int`                  | Required             | Decimal places allowed for prices.         |
| `size_precision`      | `u8`               | `int`                  | Required             | Decimal places allowed for order sizes.    |
| `price_increment`     | `Price`            | `Price`                | Required             | Smallest valid price step.                 |
| `size_increment`      | `Quantity`         | `Quantity`             | Required             | Smallest valid size step.                  |
| `ts_event`            | `UnixNanos`        | `int`                  | Required             | Event timestamp in nanoseconds.            |
| `ts_init`             | `UnixNanos`        | `int`                  | Required             | Initialization timestamp in nanoseconds.   |
| `multiplier`          | `Quantity`         | `Quantity`             | `1`                  | Contract multiplier.                       |
| `lot_size`            | `Quantity`         | `Quantity`             | `1`                  | Rounded lot or board size.                 |
| `max_quantity`        | `Option<Quantity>` | `Quantity \| None`      | `None`               | Maximum order quantity.                    |
| `min_quantity`        | `Option<Quantity>` | `Quantity \| None`      | `None`               | Minimum order quantity.                    |
| `max_notional`        | `Option<Money>`    | `Money \| None`         | `None`               | Maximum order notional value.              |
| `min_notional`        | `Option<Money>`    | `Money \| None`         | `None`               | Minimum order notional value.              |
| `max_price`           | `Option<Price>`    | `Price \| None`         | `None`               | Maximum valid quote or order price.        |
| `min_price`           | `Option<Price>`    | `Price \| None`         | `None`               | Minimum valid quote or order price.        |
| `margin_init`         | `Option<Decimal>`  | `Decimal \| None`       | `0`                  | Initial margin rate.                       |
| `margin_maint`        | `Option<Decimal>`  | `Decimal \| None`       | `0`                  | Maintenance margin rate.                   |
| `maker_fee`           | `Option<Decimal>`  | `Decimal \| None`       | `0`                  | Maker fee rate. Negative values rebate.    |
| `taker_fee`           | `Option<Decimal>`  | `Decimal \| None`       | `0`                  | Taker fee rate. Negative values rebate.    |
| `tick_scheme_name`    | N/A                | `str \| None`           | `None`               | Registered variable tick scheme name.      |
| `info`                | `Option<Params>`   | `dict \| None`          | `None`               | Adapter metadata.                          |

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

<Tabs items={['Rust', 'Python']}>
<Tab value="Rust">

```rust
use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::{InstrumentId, Symbol},
    instruments::{CryptoPerpetual, InstrumentAny},
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal_macros::dec;

let ethusdt_perp = CryptoPerpetual::new(
    InstrumentId::from("ETHUSDT-PERP.BINANCE"),
    Symbol::from("ETHUSDT"),
    Currency::from("ETH"),
    Currency::from("USDT"),
    Currency::from("USDT"),
    false,
    2,
    3,
    Price::from("0.01"),
    Quantity::from("0.001"),
    None,
    None,
    Some(Quantity::from("10000.000")),
    Some(Quantity::from("0.001")),
    None,
    Some(Money::from("10.00 USDT")),
    Some(Price::from("15000.00")),
    Some(Price::from("1.00")),
    Some(dec!(1.0)),
    Some(dec!(0.35)),
    Some(dec!(0.0002)),
    Some(dec!(0.0004)),
    None,
    UnixNanos::default(),
    UnixNanos::default(),
);

let instrument = InstrumentAny::CryptoPerpetual(ethusdt_perp);
```

</Tab>
<Tab value="Python">

```python
from decimal import Decimal

from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity

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
    min_notional=Money.from_str("10.00 USDT"),
    max_price=Price.from_str("15000.00"),
    min_price=Price.from_str("1.00"),
    margin_init=Decimal("1.0"),
    margin_maint=Decimal("0.35"),
    maker_fee=Decimal("0.0002"),
    taker_fee=Decimal("0.0004"),
)
```

</Tab>
</Tabs>

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
