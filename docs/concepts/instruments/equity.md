# Equity

`Equity` represents a listed share, ETF, or similar cash-market security. Nautilus uses
this type for instruments that trade in whole units, quote in one currency, and have no
contract expiry.

Examples include `AAPL.XNAS`, `MSFT.XNAS`, and venue-specific ETF symbols.

## Fields

<Tabs items={["Rust", "Python"]}>
<Tab value="Rust">

| Field             | Type               | Required/default | Notes                                    |
|-------------------|--------------------|------------------|------------------------------------------|
| `instrument_id`   | `InstrumentId`     | Required         | Stored as `id` in Rust.                  |
| `raw_symbol`      | `Symbol`           | Required         | Native venue symbol.                     |
| `currency`        | `Currency`         | Required         | Quote and settlement currency.           |
| `price_precision` | `u8`               | Required         | Decimal places allowed for prices.       |
| `price_increment` | `Price`            | Required         | Smallest valid price step.               |
| `lot_size`        | `Option<Quantity>` | `None`           | Board lot or whole‑share lot size.       |
| `ts_event`        | `UnixNanos`        | Required         | Event timestamp in nanoseconds.          |
| `ts_init`         | `UnixNanos`        | Required         | Initialization timestamp in nanoseconds. |
| `isin`            | `Option<Ustr>`     | `None`           | International Securities ID when known.  |
| `max_quantity`    | `Option<Quantity>` | `None`           | Maximum order quantity.                  |
| `min_quantity`    | `Option<Quantity>` | `None`           | Minimum order quantity.                  |
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
| `currency`        | `Currency`         | Required         | Quote and settlement currency.           |
| `price_precision` | `int`              | Required         | Decimal places allowed for prices.       |
| `price_increment` | `Price`            | Required         | Smallest valid price step.               |
| `lot_size`        | `Quantity \| None` | `None`           | Board lot or whole‑share lot size.       |
| `ts_event`        | `int`              | Required         | Event timestamp in nanoseconds.          |
| `ts_init`         | `int`              | Required         | Initialization timestamp in nanoseconds. |
| `isin`            | `str \| None`      | `None`           | International Securities ID when known.  |
| `max_quantity`    | `Quantity \| None` | `None`           | Maximum order quantity.                  |
| `min_quantity`    | `Quantity \| None` | `None`           | Minimum order quantity.                  |
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

- `Equity` has asset class `Equity` and instrument class `Spot`.
- Quantity precision is always zero, so orders use whole-share quantities.
- The multiplier and size increment are one.
- It has no base currency, expiry, strike, option kind, or inverse costing flag.
- Use price limits only when the venue publishes them.

## Example

```rust tab="Rust"
use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::{InstrumentId, Symbol},
    instruments::Equity,
    types::{Currency, Price, Quantity},
};
use ustr::Ustr;

let aapl = Equity::builder()
    .instrument_id(InstrumentId::from("AAPL.XNAS"))
    .raw_symbol(Symbol::from("AAPL"))
    .isin(Ustr::from("US0378331005"))
    .currency(Currency::from("USD"))
    .price_precision(2)
    .price_increment(Price::from("0.01"))
    .lot_size(Quantity::from("100"))
    .ts_event(UnixNanos::default())
    .ts_init(UnixNanos::default())
    .build()
    .unwrap();
```

```python tab="Python"
from nautilus_trader.model import Currency
from nautilus_trader.model import Equity
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import Symbol

aapl = Equity(
    instrument_id=InstrumentId.from_str("AAPL.XNAS"),
    raw_symbol=Symbol("AAPL"),
    currency=Currency.from_str("USD"),
    price_precision=2,
    price_increment=Price.from_str("0.01"),
    ts_event=0,
    ts_init=0,
    isin="US0378331005",
    lot_size=Quantity.from_int(100),
)
```

## Adapters

Representative adapters that create or consume `Equity` instruments include:

- [Databento](../../integrations/databento.md) for listed US equities and ETFs.
- [Interactive Brokers](../../integrations/ib.md) for listed equity contracts.

## Related guides

- [Data](../data.md) explains market data that references instruments.
- [Value types](../value_types.md) explains `Price`, `Quantity`, and `Money`.
