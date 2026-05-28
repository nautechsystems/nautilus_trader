# Crypto Future

`CryptoFuture` represents a dated crypto futures contract. It tracks a crypto
underlying, quotes in a quote currency, settles in a settlement currency, and expires at
a fixed timestamp.

Examples include dated BTC or ETH futures on crypto derivatives venues.

## Fields

| Field                 | Rust type          | Python type       | Required/default | Notes                                   |
|-----------------------|--------------------|-------------------|------------------|-----------------------------------------|
| `instrument_id`       | `InstrumentId`     | `InstrumentId`    | Required         | Stored as `id` in Rust.                 |
| `raw_symbol`          | `Symbol`           | `Symbol`          | Required         | Native venue symbol.                    |
| `underlying`          | `Currency`         | `Currency`        | Required         | Crypto asset the contract tracks.       |
| `quote_currency`      | `Currency`         | `Currency`        | Required         | Currency used to quote the price.       |
| `settlement_currency` | `Currency`         | `Currency`        | Required         | Currency used to settle PnL and fees.   |
| `is_inverse`          | `bool`             | `bool`            | Required         | True when sizing/costing is inverse.    |
| `activation_ns`       | `UnixNanos`        | `int`             | Required         | Contract activation timestamp.          |
| `expiration_ns`       | `UnixNanos`        | `int`             | Required         | Contract expiration timestamp.          |
| `price_precision`     | `u8`               | `int`             | Required         | Decimal places allowed for prices.      |
| `size_precision`      | `u8`               | `int`             | Required         | Decimal places allowed for order sizes. |
| `price_increment`     | `Price`            | `Price`           | Required         | Smallest valid price step.              |
| `size_increment`      | `Quantity`         | `Quantity`        | Required         | Smallest valid size step.               |
| `multiplier`          | `Quantity`         | `Quantity`        | `1`              | Contract multiplier.                    |
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

- `CryptoFuture` has asset class `Cryptocurrency` and instrument class `Future`.
- Linear contracts typically set `is_inverse=False` and settle in the quote currency.
- Inverse contracts set `is_inverse=True` and typically settle in the underlying currency.
- Quanto contracts settle in a third currency that differs from both underlying and quote.
- Use `CryptoPerpetual` for crypto derivatives with no expiration.

## Example

<Tabs items={['Rust', 'Python']}>
<Tab value="Rust">

```rust
use nautilus_model::instruments::CryptoFuture;

fn settlement_pair(instrument: &CryptoFuture) -> String {
    format!("{}/{}", instrument.quote_currency, instrument.settlement_currency)
}
```

</Tab>
<Tab value="Python">

```python
from nautilus_trader.model.instruments import CryptoFuture


def settlement_pair(instrument: CryptoFuture) -> str:
    return f"{instrument.quote_currency}/{instrument.settlement_currency}"
```

</Tab>
</Tabs>

## Adapters

Representative adapters that create or consume `CryptoFuture` instruments include:

- [BitMEX](../../integrations/bitmex.md) for inverse and linear dated futures.
- [Bybit](../../integrations/bybit.md) for crypto futures markets.
- [Deribit](../../integrations/deribit.md) for dated crypto futures.
- [OKX](../../integrations/okx.md) for dated crypto futures.
- [Tardis](../../integrations/tardis.md) for crypto futures metadata.

## Related guides

- [Crypto Perpetual](crypto_perpetual.md) covers perpetual crypto futures.
- [Futures Contract](futures_contract.md) covers non-crypto futures contracts.
