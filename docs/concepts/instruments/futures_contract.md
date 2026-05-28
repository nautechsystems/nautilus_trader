# Futures Contract

`FuturesContract` represents a dated, exchange-traded futures contract with a defined
underlying, activation time, expiration time, currency, multiplier, and lot size.

Examples include equity index futures, commodity futures, interest-rate futures, and
currency futures.

## Fields

| Field              | Rust type          | Python type       | Required/default | Notes                                   |
|--------------------|--------------------|-------------------|------------------|-----------------------------------------|
| `instrument_id`    | `InstrumentId`     | `InstrumentId`    | Required         | Stored as `id` in Rust.                 |
| `raw_symbol`       | `Symbol`           | `Symbol`          | Required         | Native venue symbol.                    |
| `asset_class`      | `AssetClass`       | `AssetClass`      | Required         | Asset class of the underlying.          |
| `exchange`         | `Option<Ustr>`     | `str \| None`      | `None`           | Exchange MIC or venue code when known.  |
| `underlying`       | `Ustr`             | `str`             | Required         | Underlying asset, index, or product.    |
| `activation_ns`    | `UnixNanos`        | `int`             | Required         | Contract activation timestamp.          |
| `expiration_ns`    | `UnixNanos`        | `int`             | Required         | Contract expiration timestamp.          |
| `currency`         | `Currency`         | `Currency`        | Required         | Quote and settlement currency.          |
| `price_precision`  | `u8`               | `int`             | Required         | Decimal places allowed for prices.      |
| `price_increment`  | `Price`            | `Price`           | Required         | Smallest valid price step.              |
| `size_precision`   | `u8`               | `int`             | `0`              | Futures trade in whole contracts.       |
| `size_increment`   | `Quantity`         | `Quantity`        | `1`              | Minimum contract size step.             |
| `multiplier`       | `Quantity`         | `Quantity`        | Required         | Contract multiplier.                    |
| `lot_size`         | `Quantity`         | `Quantity`        | Required         | Rounded lot or contract lot size.       |
| `margin_init`      | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Initial margin rate.                    |
| `margin_maint`     | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Maintenance margin rate.                |
| `maker_fee`        | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Maker fee rate. Negative values rebate. |
| `taker_fee`        | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Taker fee rate. Negative values rebate. |
| `max_quantity`     | `Option<Quantity>` | `Quantity \| None` | `None`           | Maximum order quantity.                 |
| `min_quantity`     | `Option<Quantity>` | `Quantity \| None` | `1`              | Minimum order quantity.                 |
| `max_price`        | `Option<Price>`    | `Price \| None`    | `None`           | Maximum valid quote or order price.     |
| `min_price`        | `Option<Price>`    | `Price \| None`    | `None`           | Minimum valid quote or order price.     |
| `tick_scheme_name` | N/A                | `str \| None`      | `None`           | Registered variable tick scheme name.   |
| `info`             | `Option<Params>`   | `dict \| None`     | `None`           | Adapter metadata.                       |
| `ts_event`         | `UnixNanos`        | `int`             | Required         | Event timestamp in nanoseconds.         |
| `ts_init`          | `UnixNanos`        | `int`             | Required         | Initialization timestamp in nanoseconds. |

*Note: Python constructors use `instrument_id`; Rust stores the same value as `id`.*

## Behavior

- `FuturesContract` has instrument class `Future`.
- It is never inverse. Cost, settlement, and quote currency use `currency`.
- It trades in whole contracts with size precision `0` and size increment `1`.
- Use `CryptoFuture` for dated crypto futures where the underlying and settlement
  currencies can differ.

## Example

```rust tab="Rust"
use nautilus_model::instruments::FuturesContract;

fn contract_term(instrument: &FuturesContract) -> (u64, u64) {
    (instrument.activation_ns.as_u64(), instrument.expiration_ns.as_u64())
}
```

```python tab="Python"
from nautilus_trader.model.instruments import FuturesContract


def contract_term(instrument: FuturesContract) -> tuple[int, int]:
    return instrument.activation_ns, instrument.expiration_ns
```

## Adapters

Representative adapters that create or consume `FuturesContract` instruments include:

- [Databento](../../integrations/databento.md) for futures reference data and market data.
- [Interactive Brokers](../../integrations/ib.md) for listed futures contracts.

## Related guides

- [Continuous Futures](../continuous_futures.md) covers roll-adjusted futures series.
- [Crypto Future](crypto_future.md) covers dated crypto futures contracts.
