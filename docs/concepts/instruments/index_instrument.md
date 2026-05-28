# Index Instrument

`IndexInstrument` represents a reference index such as an equity index, volatility index,
or benchmark price series. It carries precision and increment metadata so Nautilus can
store and route prices consistently, but it is not a directly tradable contract.

Examples include `SPX.XCBO`, `VIX.XCBO`, and venue-specific reference indexes.

## Fields

| Field              | Rust type        | Python type    | Required/default | Notes                                    |
|--------------------|------------------|----------------|------------------|------------------------------------------|
| `instrument_id`    | `InstrumentId`   | `InstrumentId` | Required         | Stored as `id` in Rust.                  |
| `raw_symbol`       | `Symbol`         | `Symbol`       | Required         | Native venue symbol.                     |
| `currency`         | `Currency`       | `Currency`     | Required         | Reference currency for quoted values.    |
| `price_precision`  | `u8`             | `int`          | Required         | Decimal places allowed for prices.       |
| `size_precision`   | `u8`             | `int`          | Required         | Decimal places allowed for quantities.   |
| `price_increment`  | `Price`          | `Price`        | Required         | Smallest valid price step.               |
| `size_increment`   | `Quantity`       | `Quantity`     | Required         | Smallest valid size step.                |
| `ts_event`         | `UnixNanos`      | `int`          | Required         | Event timestamp in nanoseconds.          |
| `ts_init`          | `UnixNanos`      | `int`          | Required         | Initialization timestamp in nanoseconds. |
| `tick_scheme_name` | N/A              | `str \| None`   | `None`           | Registered variable tick scheme name.    |
| `info`             | `Option<Params>` | `dict \| None`  | `None`           | Adapter metadata.                        |

*Note: Python constructors use `instrument_id`; Rust stores the same value as `id`.*

## Behavior

- `IndexInstrument` has asset class `Index` and instrument class `Spot`.
- It is a reference instrument and should not be used for order submission.
- It has no limits, margins, fees, contract multiplier, expiry, or settlement currency.
- Use option or futures types for tradable derivatives whose underlyings are indexes.

## Example

```rust tab="Rust"
use nautilus_model::instruments::IndexInstrument;

fn index_label(instrument: &IndexInstrument) -> String {
    format!("{} {}", instrument.raw_symbol, instrument.currency)
}
```

```python tab="Python"
from nautilus_trader.model.instruments import IndexInstrument


def index_label(instrument: IndexInstrument) -> str:
    return f"{instrument.raw_symbol} {instrument.currency}"
```

## Adapters

Representative adapters that create or consume `IndexInstrument` instruments include:

- [Interactive Brokers](../../integrations/ib.md) for reference indexes.
- [Databento](../../integrations/databento.md) for reference data feeds.

## Related guides

- [Option Contract](option_contract.md) covers listed options on index underlyings.
- [Futures Contract](futures_contract.md) covers index futures.
