# Synthetic Instrument

`SyntheticInstrument` represents a local instrument whose price comes from a formula over
other instruments. It is useful for spreads, baskets, ratios, and other derived prices
that should appear in the system as an instrument.

Examples include `(BTC.BINANCE + LTC.BINANCE) / 2.0` and ratio-style pairs built from
component instrument prices.

## Fields

| Field             | Rust type           | Python type          | Required/default | Notes                                       |
|-------------------|---------------------|----------------------|------------------|---------------------------------------------|
| `symbol`          | `Symbol`            | `Symbol`             | Required         | Synthetic symbol used with venue `SYNTH`.   |
| `id`              | `InstrumentId`      | `InstrumentId`       | Derived          | Instrument ID formed from `symbol.SYNTH`.   |
| `price_precision` | `u8`                | `int`                | Required         | Decimal places allowed for synthetic price. |
| `price_increment` | `Price`             | `Price`              | Derived          | Smallest price step from precision.         |
| `components`      | `Vec<InstrumentId>` | `list[InstrumentId]` | Required         | Component instruments used by the formula.  |
| `formula`         | `String`            | `str`                | Required         | Numeric expression over component IDs.      |
| `ts_event`        | `UnixNanos`         | `int`                | Required         | Event timestamp in nanoseconds.             |
| `ts_init`         | `UnixNanos`         | `int`                | Required         | Initialization timestamp in nanoseconds.    |

*Note: Python constructs the instrument ID from `symbol` and the `SYNTH` venue. Rust
stores the same value as `id`.*

## Behavior

- `SyntheticInstrument` is local to Nautilus and does not represent a venue orderable market.
- It always uses the synthetic venue `SYNTH`.
- Python requires at least two component instrument IDs.
- The formula must compile against the supplied component identifiers before the object is valid.
- It has no venue limits, margins, fees, order book, or adapter-specific metadata.

## Example

```rust tab="Rust"
use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::{InstrumentId, Symbol},
    instruments::SyntheticInstrument,
};

let synthetic = SyntheticInstrument::new(
    Symbol::from("BTC-LTC"),
    2,
    vec![
        InstrumentId::from("BTC.BINANCE"),
        InstrumentId::from("LTC.BINANCE"),
    ],
    "(BTC.BINANCE + LTC.BINANCE) / 2.0",
    UnixNanos::default(),
    UnixNanos::default(),
);
```

```python tab="Python"
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Symbol
from nautilus_trader.model import SyntheticInstrument

synthetic = SyntheticInstrument(
    symbol=Symbol("BTC-LTC"),
    price_precision=2,
    components=[
        InstrumentId.from_str("BTC.BINANCE"),
        InstrumentId.from_str("LTC.BINANCE"),
    ],
    formula="(BTC.BINANCE + LTC.BINANCE) / 2.0",
    ts_event=0,
    ts_init=0,
)
```

## Adapters

`SyntheticInstrument` is local only. It derives prices from component instruments that
may come from any adapter already loaded into the system.

## Related guides

- [Synthetics](../synthetics.md) covers formula-derived instruments and synthetic bars.
- [Data](../data.md) explains market data that references instruments.
