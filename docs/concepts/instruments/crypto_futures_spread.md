# Crypto Futures Spread

`CryptoFuturesSpread` represents an exchange-defined spread strategy over crypto
futures. The venue publishes the strategy as a single instrument with its own symbol,
strategy type, precision, increments, and expiration.

Examples include listed crypto futures calendar spreads.

## Fields

| Field                 | Rust type          | Python type       | Required/default | Notes                                   |
|-----------------------|--------------------|-------------------|------------------|-----------------------------------------|
| `instrument_id`       | `InstrumentId`     | `InstrumentId`    | Required         | Stored as `id` in Rust.                 |
| `raw_symbol`          | `Symbol`           | `Symbol`          | Required         | Native venue symbol.                    |
| `underlying`          | `Currency`         | `Currency`        | Required         | Crypto asset the strategy tracks.       |
| `quote_currency`      | `Currency`         | `Currency`        | Required         | Currency used to quote the price.       |
| `settlement_currency` | `Currency`         | `Currency`        | Required         | Currency used to settle PnL and fees.   |
| `is_inverse`          | `bool`             | `bool`            | Required         | True when sizing/costing is inverse.    |
| `strategy_type`       | `Ustr`             | `str`             | Required         | Venue strategy type, such as calendar.  |
| `activation_ns`       | `UnixNanos`        | `int`             | Required         | Strategy activation timestamp.          |
| `expiration_ns`       | `UnixNanos`        | `int`             | Required         | Strategy expiration timestamp.          |
| `price_precision`     | `u8`               | `int`             | Required         | Decimal places allowed for prices.      |
| `size_precision`      | `u8`               | `int`             | Required         | Decimal places allowed for order sizes. |
| `price_increment`     | `Price`            | `Price`           | Required         | Smallest valid price step.              |
| `size_increment`      | `Quantity`         | `Quantity`        | Required         | Smallest valid size step.               |
| `multiplier`          | `Quantity`         | `Quantity`        | `1`              | Strategy multiplier.                    |
| `lot_size`            | `Quantity`         | `Quantity`        | `1`              | Rounded lot or board size.              |
| `max_quantity`        | `Option<Quantity>` | `Quantity \| None` | `None`           | Maximum order quantity.                 |
| `min_quantity`        | `Option<Quantity>` | `Quantity \| None` | `None`           | Minimum order quantity.                 |
| `max_notional`        | `Option<Money>`    | `Money \| None`    | `None`           | Maximum order notional value.           |
| `min_notional`        | `Option<Money>`    | `Money \| None`    | `None`           | Minimum order notional value.           |
| `max_price`           | `Option<Price>`    | `Price \| None`    | `None`           | Maximum valid quote or order price.     |
| `min_price`           | `Option<Price>`    | `Price \| None`    | `None`           | Minimum valid quote or order price.     |
| `margin_init`         | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Initial margin rate.                    |
| `margin_maint`        | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Maintenance margin rate.                |
| `maker_fee`           | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Maker fee rate. Negative values rebate. |
| `taker_fee`           | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Taker fee rate. Negative values rebate. |
| `tick_scheme_name`    | N/A                | `str \| None`      | `None`           | Registered variable tick scheme name.   |
| `info`                | `Option<Params>`   | `dict \| None`     | `None`           | Adapter metadata.                       |
| `ts_event`            | `UnixNanos`        | `int`             | Required         | Event timestamp in nanoseconds.         |
| `ts_init`             | `UnixNanos`        | `int`             | Required         | Initialization timestamp in nanoseconds. |

*Note: Python constructors use `instrument_id`; Rust stores the same value as `id`.*

## Behavior

- `CryptoFuturesSpread` has asset class `Cryptocurrency` and instrument class
  `FuturesSpread`.
- The venue publishes the spread as a single tradable instrument.
- The strategy can be linear, inverse, or quanto, depending on the currency set.
- Store venue-specific leg details in `info` when the adapter provides them.

## Example

<Tabs items={['Rust', 'Python']}>
<Tab value="Rust">

```rust
use nautilus_model::instruments::CryptoFuturesSpread;

fn spread_summary(instrument: &CryptoFuturesSpread) -> String {
    format!("{} {}", instrument.underlying, instrument.strategy_type)
}
```

</Tab>
<Tab value="Python">

```python
from nautilus_trader.model.instruments import CryptoFuturesSpread


def spread_summary(instrument: CryptoFuturesSpread) -> str:
    return f"{instrument.underlying} {instrument.strategy_type}"
```

</Tab>
</Tabs>

## Adapters

Representative adapters that create or consume `CryptoFuturesSpread` instruments include:

- [Deribit](../../integrations/deribit.md) for crypto futures combos.
- [OKX](../../integrations/okx.md) for crypto futures spread markets.

## Related guides

- [Crypto Future](crypto_future.md) covers single-leg dated crypto futures.
- [Futures Spread](futures_spread.md) covers non-crypto futures spreads.
