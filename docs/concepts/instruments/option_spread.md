# Option Spread

`OptionSpread` represents an exchange-defined options strategy with more than one leg.
The venue publishes the strategy as a single instrument with its own symbol, tick size,
expiration, and execution rules.

Examples include listed vertical spreads, calendar spreads, and other option strategies.

## Fields

| Field              | Rust type          | Python type       | Required/default | Notes                                      |
|--------------------|--------------------|-------------------|------------------|--------------------------------------------|
| `instrument_id`    | `InstrumentId`     | `InstrumentId`    | Required         | Stored as `id` in Rust.                    |
| `raw_symbol`       | `Symbol`           | `Symbol`          | Required         | Native venue symbol.                       |
| `asset_class`      | `AssetClass`       | `AssetClass`      | Required         | Asset class of the underlying strategy.    |
| `exchange`         | `Option<Ustr>`     | `str \| None`      | `None`           | Exchange MIC or venue code when known.     |
| `underlying`       | `Ustr`             | `str`             | Required         | Underlying asset, future, or index.        |
| `strategy_type`    | `Ustr`             | `str`             | Required         | Venue strategy type, such as vertical.     |
| `activation_ns`    | `UnixNanos`        | `int`             | Required         | Strategy activation timestamp.             |
| `expiration_ns`    | `UnixNanos`        | `int`             | Required         | Strategy expiration timestamp.             |
| `currency`         | `Currency`         | `Currency`        | Required         | Premium quote and settlement currency.     |
| `price_precision`  | `u8`               | `int`             | Required         | Decimal places allowed for prices.         |
| `price_increment`  | `Price`            | `Price`           | Required         | Smallest valid price step.                 |
| `size_precision`   | `u8`               | `int`             | `0`              | Option spreads trade in whole contracts.   |
| `size_increment`   | `Quantity`         | `Quantity`        | `1`              | Minimum contract size step.                |
| `multiplier`       | `Quantity`         | `Quantity`        | Required         | Strategy multiplier.                       |
| `lot_size`         | `Quantity`         | `Quantity`        | Required         | Rounded lot or contract lot size.          |
| `margin_init`      | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Initial margin rate.                       |
| `margin_maint`     | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Maintenance margin rate.                   |
| `maker_fee`        | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Maker fee rate. Negative values rebate.    |
| `taker_fee`        | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Taker fee rate. Negative values rebate.    |
| `max_quantity`     | `Option<Quantity>` | `Quantity \| None` | `None`           | Maximum order quantity.                    |
| `min_quantity`     | `Option<Quantity>` | `Quantity \| None` | `1`              | Minimum order quantity.                    |
| `max_price`        | `Option<Price>`    | `Price \| None`    | `None`           | Maximum valid quote or order price.        |
| `min_price`        | `Option<Price>`    | `Price \| None`    | `None`           | Minimum valid quote or order price.        |
| `tick_scheme_name` | N/A                | `str \| None`      | `None`           | Registered variable tick scheme name.      |
| `info`             | `Option<Params>`   | `dict \| None`     | `None`           | Adapter metadata.                          |
| `ts_event`         | `UnixNanos`        | `int`             | Required         | Event timestamp in nanoseconds.            |
| `ts_init`          | `UnixNanos`        | `int`             | Required         | Initialization timestamp in nanoseconds.   |

*Note: Python constructors use `instrument_id`; Rust stores the same value as `id`.*

## Behavior

- `OptionSpread` has instrument class `OptionSpread`.
- The venue publishes the spread as a single tradable instrument.
- It trades in whole contracts with size precision `0` and size increment `1`.
- Store venue-specific leg details in `info` when the adapter provides them.

## Example

```rust tab="Rust"
use nautilus_model::instruments::OptionSpread;

fn spread_label(instrument: &OptionSpread) -> String {
    format!("{} {}", instrument.underlying, instrument.strategy_type)
}
```

```python tab="Python"
from nautilus_trader.model.instruments import OptionSpread


def spread_label(instrument: OptionSpread) -> str:
    return f"{instrument.underlying} {instrument.strategy_type}"
```

## Adapters

Representative adapters that create or consume `OptionSpread` instruments include:

- [Databento](../../integrations/databento.md) for listed option spread markets.
- [Interactive Brokers](../../integrations/ib.md) for exchange-defined option strategies.

## Related guides

- [Option Contract](option_contract.md) covers single-leg option contracts.
- [Options](../options.md) covers option data, Greeks, and chain subscriptions.
