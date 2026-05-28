# Currency Pair

`CurrencyPair` represents a spot or cash market quoted as `BASE/QUOTE`. The base
currency is the asset being bought or sold, and the quote currency prices one unit of
the base. Nautilus uses this type for fiat FX pairs and crypto spot pairs.

Examples include `EUR/USD.SIM`, `BTCUSDT.BINANCE`, and `ETH/USD.KRAKEN`.

## Fields

| Field               | Rust type          | Python type            | Required/default     | Notes                                      |
|---------------------|--------------------|------------------------|----------------------|--------------------------------------------|
| `instrument_id`     | `InstrumentId`     | `InstrumentId`         | Required             | Stored as `id` in Rust.                    |
| `raw_symbol`        | `Symbol`           | `Symbol`               | Required             | Native venue symbol.                       |
| `base_currency`     | `Currency`         | `Currency`             | Required             | Asset bought or sold.                      |
| `quote_currency`    | `Currency`         | `Currency`             | Required             | Currency used to price the base asset.     |
| `price_precision`   | `u8`               | `int`                  | Required             | Decimal places allowed for prices.         |
| `size_precision`    | `u8`               | `int`                  | Required             | Decimal places allowed for order sizes.    |
| `price_increment`   | `Price`            | `Price`                | Required             | Smallest valid price step.                 |
| `size_increment`    | `Quantity`         | `Quantity`             | Required             | Smallest valid size step.                  |
| `ts_event`          | `UnixNanos`        | `int`                  | Required             | Event timestamp in nanoseconds.            |
| `ts_init`           | `UnixNanos`        | `int`                  | Required             | Initialization timestamp in nanoseconds.   |
| `multiplier`        | `Quantity`         | `Quantity`             | `1`                  | Contract multiplier.                       |
| `lot_size`          | `Option<Quantity>` | `Quantity \| None`      | `None`               | Rounded lot or board size.                 |
| `max_quantity`      | `Option<Quantity>` | `Quantity \| None`      | `None`               | Maximum order quantity.                    |
| `min_quantity`      | `Option<Quantity>` | `Quantity \| None`      | `None`               | Minimum order quantity.                    |
| `max_notional`      | `Option<Money>`    | `Money \| None`         | `None`               | Maximum order notional value.              |
| `min_notional`      | `Option<Money>`    | `Money \| None`         | `None`               | Minimum order notional value.              |
| `max_price`         | `Option<Price>`    | `Price \| None`         | `None`               | Maximum valid quote or order price.        |
| `min_price`         | `Option<Price>`    | `Price \| None`         | `None`               | Minimum valid quote or order price.        |
| `margin_init`       | `Option<Decimal>`  | `Decimal \| None`       | `0`                  | Initial margin rate.                       |
| `margin_maint`      | `Option<Decimal>`  | `Decimal \| None`       | `0`                  | Maintenance margin rate.                   |
| `maker_fee`         | `Option<Decimal>`  | `Decimal \| None`       | `0`                  | Maker fee rate. Negative values rebate.    |
| `taker_fee`         | `Option<Decimal>`  | `Decimal \| None`       | `0`                  | Taker fee rate. Negative values rebate.    |
| `tick_scheme_name`  | N/A                | `str \| None`           | `None`               | Registered variable tick scheme name.      |
| `info`              | `Option<Params>`   | `dict \| None`          | `None`               | Adapter metadata.                          |

*Note: Python constructors use `instrument_id`; Rust stores the same value as `id`.*

## Behavior

- `CurrencyPair` has instrument class `Spot`.
- It has no expiration, strike price, option kind, or derivative underlying field.
- It is never inverse. The settlement currency and cost currency are the quote currency.
- Use this type for both fiat FX pairs and crypto spot pairs.

:::warning
Do not model dated futures, swaps, or options as `CurrencyPair` only because their symbols
look like pairs. Use the specific derivative type so cost currency, settlement currency,
expiration, and notional calculations match the venue.
:::

## Example

```rust tab="Rust"
use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::{InstrumentId, Symbol},
    instruments::{CurrencyPair, InstrumentAny},
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal_macros::dec;

let btcusdt = CurrencyPair::new(
    InstrumentId::from("BTCUSDT.BINANCE"),
    Symbol::from("BTCUSDT"),
    Currency::from("BTC"),
    Currency::from("USDT"),
    2,
    6,
    Price::from("0.01"),
    Quantity::from("0.000001"),
    None,
    None,
    None,
    Some(Quantity::from("0.000001")),
    None,
    Some(Money::from("10.00 USDT")),
    Some(Price::from("1000000.00")),
    Some(Price::from("0.01")),
    Some(dec!(0.001)),
    Some(dec!(0.001)),
    Some(dec!(0.001)),
    Some(dec!(0.001)),
    None,
    UnixNanos::default(),
    UnixNanos::default(),
);

let instrument = InstrumentAny::CurrencyPair(btcusdt);
```

```python tab="Python"
from decimal import Decimal

from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity

btcusdt = CurrencyPair(
    instrument_id=InstrumentId.from_str("BTCUSDT.BINANCE"),
    raw_symbol=Symbol("BTCUSDT"),
    base_currency=BTC,
    quote_currency=USDT,
    price_precision=2,
    size_precision=6,
    price_increment=Price.from_str("0.01"),
    size_increment=Quantity.from_str("0.000001"),
    ts_event=0,
    ts_init=0,
    min_quantity=Quantity.from_str("0.000001"),
    min_notional=Money.from_str("10.00 USDT"),
    max_price=Price.from_str("1000000.00"),
    min_price=Price.from_str("0.01"),
    margin_init=Decimal("0.001"),
    margin_maint=Decimal("0.001"),
    maker_fee=Decimal("0.001"),
    taker_fee=Decimal("0.001"),
)
```

## Adapters

Representative adapters that create or consume `CurrencyPair` instruments include:

- [Binance](../../integrations/binance.md) for spot markets.
- [Kraken](../../integrations/kraken.md) for spot markets.
- [OKX](../../integrations/okx.md) for spot markets.
- [Tardis](../../integrations/tardis.md) for spot metadata.
- [Interactive Brokers](../../integrations/ib.md) for FX cash contracts.
- [Hyperliquid](../../integrations/hyperliquid.md) for spot assets.

## Related guides

- [Data](../data.md) explains market data that references instruments.
- [Execution](../execution.md) explains order checks that use instrument precision.
- [Value types](../value_types.md) explains `Price`, `Quantity`, and `Money`.
