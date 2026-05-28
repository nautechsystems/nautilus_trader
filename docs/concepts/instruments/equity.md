# Equity

`Equity` represents a listed share, ETF, or similar cash-market security. Nautilus uses
this type for instruments that trade in whole units, quote in one currency, and have no
contract expiry.

Examples include `AAPL.XNAS`, `MSFT.XNAS`, and venue-specific ETF symbols.

## Fields

| Field              | Rust type          | Python type       | Required/default | Notes                                   |
|--------------------|--------------------|-------------------|------------------|-----------------------------------------|
| `instrument_id`    | `InstrumentId`     | `InstrumentId`    | Required         | Stored as `id` in Rust.                 |
| `raw_symbol`       | `Symbol`           | `Symbol`          | Required         | Native venue symbol.                    |
| `currency`         | `Currency`         | `Currency`        | Required         | Quote and settlement currency.          |
| `price_precision`  | `u8`               | `int`             | Required         | Decimal places allowed for prices.      |
| `price_increment`  | `Price`            | `Price`           | Required         | Smallest valid price step.              |
| `lot_size`         | `Option<Quantity>` | `Quantity`        | Required/Python  | Board lot or whole‑share lot size.      |
| `ts_event`         | `UnixNanos`        | `int`             | Required         | Event timestamp in nanoseconds.         |
| `ts_init`          | `UnixNanos`        | `int`             | Required         | Initialization timestamp in nanoseconds. |
| `isin`             | `Option<Ustr>`     | `str \| None`      | `None`           | International Securities ID when known. |
| `max_quantity`     | `Option<Quantity>` | `Quantity \| None` | `None`           | Maximum order quantity.                 |
| `min_quantity`     | `Option<Quantity>` | `Quantity \| None` | `None`           | Minimum order quantity.                 |
| `max_price`        | `Option<Price>`    | N/A               | Rust only        | Maximum valid quote or order price.     |
| `min_price`        | `Option<Price>`    | N/A               | Rust only        | Minimum valid quote or order price.     |
| `margin_init`      | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Initial margin rate.                    |
| `margin_maint`     | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Maintenance margin rate.                |
| `maker_fee`        | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Maker fee rate. Negative values rebate. |
| `taker_fee`        | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Taker fee rate. Negative values rebate. |
| `tick_scheme_name` | N/A                | `str \| None`      | `None`           | Registered variable tick scheme name.   |
| `info`             | `Option<Params>`   | `dict \| None`     | `None`           | Adapter metadata.                       |

*Note: Python constructors use `instrument_id`; Rust stores the same value as `id`.*

## Behavior

- `Equity` has asset class `Equity` and instrument class `Spot`.
- Quantity precision is always zero, so orders use whole-share quantities.
- The multiplier and size increment are one.
- It has no base currency, expiry, strike, option kind, or inverse costing flag.
- Use price limits only when the venue publishes them.

## Example

```rust tab="Rust"
use nautilus_model::instruments::Equity;

fn listing_summary(instrument: &Equity) -> String {
    format!("{} trades in {}", instrument.raw_symbol, instrument.currency)
}
```

```python tab="Python"
from nautilus_trader.model.instruments import Equity


def listing_summary(instrument: Equity) -> str:
    return f"{instrument.raw_symbol} trades in {instrument.currency}"
```

## Adapters

Representative adapters that create or consume `Equity` instruments include:

- [Databento](../../integrations/databento.md) for listed US equities and ETFs.
- [Interactive Brokers](../../integrations/ib.md) for listed equity contracts.

## Related guides

- [Data](../data.md) explains market data that references instruments.
- [Value types](../value_types.md) explains `Price`, `Quantity`, and `Money`.
