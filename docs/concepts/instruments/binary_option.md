# Binary Option

`BinaryOption` represents a binary outcome instrument that settles to a fixed payoff
based on whether a condition is true. It can model prediction markets, binary options,
or venue-specific yes/no contracts.

Examples include prediction market outcomes and binary event contracts.

## Fields

| Field              | Rust type          | Python type       | Required/default | Notes                                   |
|--------------------|--------------------|-------------------|------------------|-----------------------------------------|
| `instrument_id`    | `InstrumentId`     | `InstrumentId`    | Required         | Stored as `id` in Rust.                 |
| `raw_symbol`       | `Symbol`           | `Symbol`          | Required         | Native venue symbol.                    |
| `asset_class`      | `AssetClass`       | `AssetClass`      | Required         | Asset class of the outcome market.      |
| `currency`         | `Currency`         | `Currency`        | Required         | Quote and settlement currency.          |
| `activation_ns`    | `UnixNanos`        | `int`             | Required         | Contract activation timestamp.          |
| `expiration_ns`    | `UnixNanos`        | `int`             | Required         | Contract expiration timestamp.          |
| `price_precision`  | `u8`               | `int`             | Required         | Decimal places allowed for prices.      |
| `size_precision`   | `u8`               | `int`             | Required         | Decimal places allowed for order sizes. |
| `price_increment`  | `Price`            | `Price`           | Required         | Smallest valid price step.              |
| `size_increment`   | `Quantity`         | `Quantity`        | Required         | Smallest valid size step.               |
| `outcome`          | `Option<Ustr>`     | `str \| None`      | `None`           | Outcome label when the venue provides it. |
| `description`      | `Option<Ustr>`     | `str \| None`      | `None`           | Human‑readable market description.      |
| `max_quantity`     | `Option<Quantity>` | `Quantity \| None` | `None`           | Maximum order quantity.                 |
| `min_quantity`     | `Option<Quantity>` | `Quantity \| None` | `None`           | Minimum order quantity.                 |
| `max_notional`     | `Option<Money>`    | N/A               | Rust only        | Maximum order notional value.           |
| `min_notional`     | `Option<Money>`    | N/A               | Rust only        | Minimum order notional value.           |
| `max_price`        | `Option<Price>`    | N/A               | Rust only        | Maximum valid quote or order price.     |
| `min_price`        | `Option<Price>`    | N/A               | Rust only        | Minimum valid quote or order price.     |
| `margin_init`      | `Option<Decimal>`  | N/A               | Rust only        | Initial margin rate.                    |
| `margin_maint`     | `Option<Decimal>`  | N/A               | Rust only        | Maintenance margin rate.                |
| `maker_fee`        | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Maker fee rate. Negative values rebate. |
| `taker_fee`        | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Taker fee rate. Negative values rebate. |
| `tick_scheme_name` | N/A                | `str \| None`      | `None`           | Registered variable tick scheme name.   |
| `info`             | `Option<Params>`   | `dict \| None`     | `None`           | Adapter metadata.                       |
| `ts_event`         | `UnixNanos`        | `int`             | Required         | Event timestamp in nanoseconds.         |
| `ts_init`          | `UnixNanos`        | `int`             | Required         | Initialization timestamp in nanoseconds. |

*Note: Python constructors use `instrument_id`; Rust stores the same value as `id`.*

## Behavior

- `BinaryOption` has instrument class `BinaryOption`.
- It is never inverse and uses a multiplier and lot size of one.
- Many venues quote binary outcomes between zero and one, but the venue defines the
  allowed price range and tick size.
- `outcome` and `description` provide human-readable context for the contract.

## Example

```rust tab="Rust"
use nautilus_model::instruments::BinaryOption;

fn outcome_label(instrument: &BinaryOption) -> String {
    instrument.outcome.map_or("unknown".to_string(), |value| value.to_string())
}
```

```python tab="Python"
from nautilus_trader.model.instruments import BinaryOption


def outcome_label(instrument: BinaryOption) -> str:
    return instrument.outcome or "unknown"
```

## Adapters

Representative adapters that create or consume `BinaryOption` instruments include:

- [Hyperliquid](../../integrations/hyperliquid.md) for binary and prediction-style markets.
- [OKX](../../integrations/okx.md) for venue-defined binary outcome products.
- [Polymarket](../../integrations/polymarket.md) for prediction market outcomes.

## Related guides

- [Order Book](../order_book.md) covers binary market order book behavior.
- [Data](../data.md) explains market data that references instruments.
