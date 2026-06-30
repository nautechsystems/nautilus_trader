# OptionGreeks

`OptionGreeks` represents venue-provided option sensitivities and implied volatility
for one option instrument. It is a native `Data` enum variant and can be recorded,
replayed, and queried through the catalog.

## Fields

| Field              | Rust type             | Python type      | Required/default | Notes                                      |
|--------------------|-----------------------|------------------|------------------|--------------------------------------------|
| `instrument_id`    | `InstrumentId`        | `InstrumentId`   | Required         | Option instrument for the Greeks.          |
| `convention`       | `GreeksConvention`    | `GreeksConvention` | Default        | Numeraire convention for the values.       |
| `greeks`           | `OptionGreekValues`   | Separate floats  | Required         | Delta, gamma, vega, theta, and rho.        |
| `mark_iv`          | `Option<f64>`         | `float \| None`  | `None`           | Mark implied volatility.                   |
| `bid_iv`           | `Option<f64>`         | `float \| None`  | `None`           | Bid implied volatility.                    |
| `ask_iv`           | `Option<f64>`         | `float \| None`  | `None`           | Ask implied volatility.                    |
| `underlying_price` | `Option<f64>`         | `float \| None`  | `None`           | Underlying price used for the calculation. |
| `open_interest`    | `Option<f64>`         | `float \| None`  | `None`           | Open interest when published.              |
| `ts_event`         | `UnixNanos`           | `int`            | Required         | Event timestamp in nanoseconds.            |
| `ts_init`          | `UnixNanos`           | `int`            | Required         | Initialization timestamp in nanoseconds.   |

## Behavior

- `OptionGreeks` dereferences to its core `OptionGreekValues` on the Rust surface.
- The Python constructor accepts `delta`, `gamma`, `vega`, `theta`, and optional `rho`
  as separate float arguments.
- Option chain subscriptions use `underlying_price` and deltas to resolve ATM and
  delta-based strike windows.

## Example

```rust tab="Rust"
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{OptionGreekValues, OptionGreeks},
    enums::GreeksConvention,
    identifiers::InstrumentId,
};

let greeks = OptionGreeks {
    instrument_id: InstrumentId::from("BTC-20240628-65000-C.DERIBIT"),
    convention: GreeksConvention::PriceAdjusted,
    greeks: OptionGreekValues {
        delta: 0.51,
        gamma: 0.0002,
        vega: 12.5,
        theta: -3.2,
        rho: 0.1,
    },
    mark_iv: Some(0.55),
    bid_iv: Some(0.54),
    ask_iv: Some(0.56),
    underlying_price: Some(65_000.0),
    open_interest: Some(120.0),
    ts_event: UnixNanos::from(1_000_000_000),
    ts_init: UnixNanos::from(1_000_000_100),
};
```

```python tab="Python"
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OptionGreeks

greeks = OptionGreeks(
    instrument_id=InstrumentId.from_str("BTC-20240628-65000-C.DERIBIT"),
    delta=0.51,
    gamma=0.0002,
    vega=12.5,
    theta=-3.2,
    rho=0.1,
    mark_iv=0.55,
    bid_iv=0.54,
    ask_iv=0.56,
    underlying_price=65_000.0,
    open_interest=120.0,
    ts_event=1_000_000_000,
    ts_init=1_000_000_100,
)
```

## Related guides

- [Greeks](../greeks.md) covers venue-provided and locally computed Greeks.
- [Options](../options.md#optiongreeks-data-type) covers option chain subscriptions.
- [Python API Reference](/docs/python-api-latest/model/data.html) lists Python members.
