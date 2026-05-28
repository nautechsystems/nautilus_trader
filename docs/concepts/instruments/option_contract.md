# Option Contract

`OptionContract` represents a listed put or call option on a non-crypto underlying. It
defines the option kind, strike price, activation time, expiration time, currency,
multiplier, and lot size.

Examples include equity options, index options, and futures options.

## Fields

| Field              | Rust type          | Python type       | Required/default | Notes                                   |
|--------------------|--------------------|-------------------|------------------|-----------------------------------------|
| `instrument_id`    | `InstrumentId`     | `InstrumentId`    | Required         | Stored as `id` in Rust.                 |
| `raw_symbol`       | `Symbol`           | `Symbol`          | Required         | Native venue symbol.                    |
| `asset_class`      | `AssetClass`       | `AssetClass`      | Required         | Asset class of the underlying.          |
| `exchange`         | `Option<Ustr>`     | `str \| None`      | `None`           | Exchange MIC or venue code when known.  |
| `underlying`       | `Ustr`             | `str`             | Required         | Underlying asset, future, or index.     |
| `option_kind`      | `OptionKind`       | `OptionKind`      | Required         | Put or call.                            |
| `strike_price`     | `Price`            | `Price`           | Required         | Option strike price.                    |
| `activation_ns`    | `UnixNanos`        | `int`             | Required         | Contract activation timestamp.          |
| `expiration_ns`    | `UnixNanos`        | `int`             | Required         | Contract expiration timestamp.          |
| `currency`         | `Currency`         | `Currency`        | Required         | Premium quote and settlement currency.  |
| `price_precision`  | `u8`               | `int`             | Required         | Decimal places allowed for prices.      |
| `price_increment`  | `Price`            | `Price`           | Required         | Smallest valid price step.              |
| `size_precision`   | `u8`               | `int`             | `0`              | Options trade in whole contracts.       |
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

- `OptionContract` has instrument class `Option`.
- It trades in whole contracts with size precision `0` and size increment `1`.
- The option kind and strike price define the payoff shape.
- Use `CryptoOption` for options where the underlying and settlement are crypto currencies.

## Example

<Tabs items={['Rust', 'Python']}>
<Tab value="Rust">

```rust
use nautilus_model::instruments::OptionContract;

fn strike_label(instrument: &OptionContract) -> String {
    format!("{} {}", instrument.option_kind, instrument.strike_price)
}
```

</Tab>
<Tab value="Python">

```python
from nautilus_trader.model.instruments import OptionContract


def strike_label(instrument: OptionContract) -> str:
    return f"{instrument.option_kind} {instrument.strike_price}"
```

</Tab>
</Tabs>

## Adapters

Representative adapters that create or consume `OptionContract` instruments include:

- [Databento](../../integrations/databento.md) for listed options data.
- [Interactive Brokers](../../integrations/ib.md) for listed option contracts.

## Related guides

- [Options](../options.md) covers option data, Greeks, and chain subscriptions.
- [Crypto Option](crypto_option.md) covers crypto option contracts.
