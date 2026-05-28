# Futures Spread

`FuturesSpread` represents an exchange-defined futures strategy with more than one leg,
such as a calendar spread or inter-commodity spread. The venue defines the strategy,
symbol, tick size, and expiry.

Examples include listed futures calendar spreads and exchange-supported spread markets.

## Fields

| Field              | Rust type          | Python type       | Required/default | Notes                                      |
|--------------------|--------------------|-------------------|------------------|--------------------------------------------|
| `instrument_id`    | `InstrumentId`     | `InstrumentId`    | Required         | Stored as `id` in Rust.                    |
| `raw_symbol`       | `Symbol`           | `Symbol`          | Required         | Native venue symbol.                       |
| `asset_class`      | `AssetClass`       | `AssetClass`      | Required         | Asset class of the underlying strategy.    |
| `exchange`         | `Option<Ustr>`     | `str \| None`      | `None`           | Exchange MIC or venue code when known.     |
| `underlying`       | `Ustr`             | `str`             | Required         | Underlying product or product family.      |
| `strategy_type`    | `Ustr`             | `str`             | Required         | Venue strategy type, such as calendar.     |
| `activation_ns`    | `UnixNanos`        | `int`             | Required         | Strategy activation timestamp.             |
| `expiration_ns`    | `UnixNanos`        | `int`             | Required         | Strategy expiration timestamp.             |
| `currency`         | `Currency`         | `Currency`        | Required         | Quote and settlement currency.             |
| `price_precision`  | `u8`               | `int`             | Required         | Decimal places allowed for prices.         |
| `price_increment`  | `Price`            | `Price`           | Required         | Smallest valid price step.                 |
| `size_precision`   | `u8`               | `int`             | `0`              | Futures spreads trade in whole contracts.  |
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

- `FuturesSpread` has instrument class `FuturesSpread`.
- The venue publishes the spread as a single tradable instrument.
- It trades in whole contracts with size precision `0` and size increment `1`.
- Use leg data from the adapter metadata when a strategy needs venue-specific leg details.

## Example

```rust tab="Rust"
use nautilus_model::instruments::FuturesSpread;

fn strategy_label(instrument: &FuturesSpread) -> String {
    format!("{} {}", instrument.underlying, instrument.strategy_type)
}
```

```python tab="Python"
from nautilus_trader.model.instruments import FuturesSpread


def strategy_label(instrument: FuturesSpread) -> str:
    return f"{instrument.underlying} {instrument.strategy_type}"
```

## Adapters

Representative adapters that create or consume `FuturesSpread` instruments include:

- [Databento](../../integrations/databento.md) for listed futures spread markets.
- [Interactive Brokers](../../integrations/ib.md) for exchange-defined futures strategies.

## Related guides

- [Futures Contract](futures_contract.md) covers single-leg futures.
- [Continuous Futures](../continuous_futures.md) covers roll-adjusted futures series.
