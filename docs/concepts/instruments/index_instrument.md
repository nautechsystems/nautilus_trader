# Index Instrument

`IndexInstrument` represents a reference index such as an equity index, volatility index,
or benchmark price series. It carries precision and increment metadata so Nautilus can
store and route prices consistently, but it is not a directly tradable contract.

Examples include `SPX.XCBO`, `VIX.XCBO`, and venue-specific reference indexes.

## Fields

<Tabs items={["Rust", "Python"]}>
<Tab value="Rust">

| Field             | Type             | Required/default | Notes                                    |
|-------------------|------------------|------------------|------------------------------------------|
| `instrument_id`   | `InstrumentId`   | Required         | Stored as `id` in Rust.                  |
| `raw_symbol`      | `Symbol`         | Required         | Native venue symbol.                     |
| `currency`        | `Currency`       | Required         | Reference currency for quoted values.    |
| `price_precision` | `u8`             | Required         | Decimal places allowed for prices.       |
| `size_precision`  | `u8`             | Required         | Decimal places allowed for quantities.   |
| `price_increment` | `Price`          | Required         | Smallest valid price step.               |
| `size_increment`  | `Quantity`       | Required         | Smallest valid size step.                |
| `ts_event`        | `UnixNanos`      | Required         | Event timestamp in nanoseconds.          |
| `ts_init`         | `UnixNanos`      | Required         | Initialization timestamp in nanoseconds. |
| `tick_scheme`     | `Option<Ustr>`   | `None`           | Registered variable tick scheme name.    |
| `info`            | `Option<Params>` | `None`           | Adapter metadata.                        |

</Tab>
<Tab value="Python">

| Field             | Type           | Required/default | Notes                                    |
|-------------------|----------------|------------------|------------------------------------------|
| `instrument_id`   | `InstrumentId` | Required         |                                          |
| `raw_symbol`      | `Symbol`       | Required         | Native venue symbol.                     |
| `currency`        | `Currency`     | Required         | Reference currency for quoted values.    |
| `price_precision` | `int`          | Required         | Decimal places allowed for prices.       |
| `size_precision`  | `int`          | Required         | Decimal places allowed for quantities.   |
| `price_increment` | `Price`        | Required         | Smallest valid price step.               |
| `size_increment`  | `Quantity`     | Required         | Smallest valid size step.                |
| `ts_event`        | `int`          | Required         | Event timestamp in nanoseconds.          |
| `ts_init`         | `int`          | Required         | Initialization timestamp in nanoseconds. |
| `tick_scheme`     | `str \| None`  | `None`           | Registered variable tick scheme name.    |
| `info`            | `dict \| None` | `None`           | Adapter metadata.                        |

</Tab>
</Tabs>

*Note: Python constructors use `instrument_id`; Rust stores the same value as `id`.*

## Behavior

- `IndexInstrument` has asset class `Index` and instrument class `Spot`.
- It is a reference instrument and should not be used for order submission.
- It has no limits, margins, fees, contract multiplier, expiry, or settlement currency.
- Use option or futures types for tradable derivatives whose underlyings are indexes.

## Example

```rust tab="Rust"
use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::{InstrumentId, Symbol},
    instruments::IndexInstrument,
    types::{Currency, Price, Quantity},
};

let spx = IndexInstrument::builder()
    .instrument_id(InstrumentId::from("SPX.XCBO"))
    .raw_symbol(Symbol::from("SPX"))
    .currency(Currency::from("USD"))
    .price_precision(2)
    .size_precision(0)
    .price_increment(Price::from("0.01"))
    .size_increment(Quantity::from("1"))
    .ts_event(UnixNanos::default())
    .ts_init(UnixNanos::default())
    .build()
    .unwrap();
```

```python tab="Python"
from nautilus_trader.model import Currency
from nautilus_trader.model import IndexInstrument
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import Symbol

spx = IndexInstrument(
    instrument_id=InstrumentId.from_str("SPX.XCBO"),
    raw_symbol=Symbol("SPX"),
    currency=Currency.from_str("USD"),
    price_precision=2,
    size_precision=0,
    price_increment=Price.from_str("0.01"),
    size_increment=Quantity.from_str("1"),
    ts_event=0,
    ts_init=0,
)
```

## Adapters

Representative adapters that create or consume `IndexInstrument` instruments include:

- [Interactive Brokers](../../integrations/ib.md) for reference indexes.
- [Databento](../../integrations/databento.md) for reference data feeds.

## Related guides

- [Option Contract](option_contract.md) covers listed options on index underlyings.
- [Futures Contract](futures_contract.md) covers index futures.
