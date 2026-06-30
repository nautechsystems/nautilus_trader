# Bar

`Bar` represents OHLCV price and volume data for a specific `BarType`. Bars can be
provided externally by a venue or data provider, aggregated internally from quote or
trade ticks, or aggregated from smaller bars.

## Fields

| Field      | Rust type   | Python type | Required/default | Notes                                    |
|------------|-------------|-------------|------------------|------------------------------------------|
| `bar_type` | `BarType`   | `BarType`   | Required         | Instrument, aggregation, price type, and source. |
| `open`     | `Price`     | `Price`     | Required         | First price in the bar interval.         |
| `high`     | `Price`     | `Price`     | Required         | Highest price in the bar interval.       |
| `low`      | `Price`     | `Price`     | Required         | Lowest price in the bar interval.        |
| `close`    | `Price`     | `Price`     | Required         | Last price in the bar interval.          |
| `volume`   | `Quantity`  | `Quantity`  | Required         | Traded volume or tick‑volume proxy.      |
| `ts_event` | `UnixNanos` | `int`       | Required         | Bar event timestamp in nanoseconds.      |
| `ts_init`  | `UnixNanos` | `int`       | Required         | Initialization timestamp in nanoseconds. |

## Behavior

- `high` must be greater than or equal to `open`, `low`, and `close`.
- `low` must be less than or equal to `open` and `close`.
- `bar_type` determines whether a bar is internal or external.
- Composite bar types use `@` syntax to identify the source bar type.

## Example

```rust tab="Rust"
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{Bar, BarType},
    types::{Price, Quantity},
};

let bar = Bar::new(
    BarType::from("AUD/USD.SIM-1-MINUTE-LAST-EXTERNAL"),
    Price::from("0.65000"),
    Price::from("0.65010"),
    Price::from("0.64990"),
    Price::from("0.65005"),
    Quantity::from("1000000"),
    UnixNanos::from(1_000_000_000),
    UnixNanos::from(1_000_000_100),
);
```

```python tab="Python"
from nautilus_trader.model import Bar
from nautilus_trader.model import BarType
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity

bar = Bar(
    bar_type=BarType.from_str("AUD/USD.SIM-1-MINUTE-LAST-EXTERNAL"),
    open=Price.from_str("0.65000"),
    high=Price.from_str("0.65010"),
    low=Price.from_str("0.64990"),
    close=Price.from_str("0.65005"),
    volume=Quantity.from_int(1_000_000),
    ts_event=1_000_000_000,
    ts_init=1_000_000_100,
)
```

## Related guides

- [Bars and aggregation](index.md#bars-and-aggregation) covers aggregation methods.
- [Bar types](index.md#bar-types) explains `BarType` string syntax.
- [Python API Reference](/docs/python-api-latest/model/data.html) lists Python members.
