# Cfd

`Cfd` represents a contract for difference that tracks an underlying asset without
transferring ownership of the underlying. The venue defines the quote currency,
precision, increments, limits, margins, and fees.

Examples include CFD contracts on FX, equities, indexes, and commodities.

## Fields

| Field              | Rust type          | Python type       | Required/default | Notes                                   |
|--------------------|--------------------|-------------------|------------------|-----------------------------------------|
| `instrument_id`    | `InstrumentId`     | `InstrumentId`    | Required         | Stored as `id` in Rust.                 |
| `raw_symbol`       | `Symbol`           | `Symbol`          | Required         | Native venue symbol.                    |
| `asset_class`      | `AssetClass`       | `AssetClass`      | Required         | Asset class of the underlying.          |
| `base_currency`    | `Option<Currency>` | `Currency \| None` | `None`           | Base currency when the CFD tracks one.  |
| `quote_currency`   | `Currency`         | `Currency`        | Required         | Currency used to quote and value prices. |
| `price_precision`  | `u8`               | `int`             | Required         | Decimal places allowed for prices.      |
| `size_precision`   | `u8`               | `int`             | Required         | Decimal places allowed for order sizes. |
| `price_increment`  | `Price`            | `Price`           | Required         | Smallest valid price step.              |
| `size_increment`   | `Quantity`         | `Quantity`        | Required         | Smallest valid size step.               |
| `lot_size`         | `Option<Quantity>` | `Quantity \| None` | `None`           | Rounded lot or board size.              |
| `max_quantity`     | `Option<Quantity>` | `Quantity \| None` | `None`           | Maximum order quantity.                 |
| `min_quantity`     | `Option<Quantity>` | `Quantity \| None` | `None`           | Minimum order quantity.                 |
| `max_notional`     | `Option<Money>`    | `Money \| None`    | `None`           | Maximum order notional value.           |
| `min_notional`     | `Option<Money>`    | `Money \| None`    | `None`           | Minimum order notional value.           |
| `max_price`        | `Option<Price>`    | `Price \| None`    | `None`           | Maximum valid quote or order price.     |
| `min_price`        | `Option<Price>`    | `Price \| None`    | `None`           | Minimum valid quote or order price.     |
| `margin_init`      | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Initial margin rate.                    |
| `margin_maint`     | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Maintenance margin rate.                |
| `maker_fee`        | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Maker fee rate. Negative values rebate. |
| `taker_fee`        | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Taker fee rate. Negative values rebate. |
| `tick_scheme_name` | N/A                | `str \| None`      | `None`           | Registered variable tick scheme name.   |
| `info`             | `Option<Params>`   | `dict \| None`     | `None`           | Adapter metadata.                       |
| `ts_event`         | `UnixNanos`        | `int`             | Required         | Event timestamp in nanoseconds.         |
| `ts_init`          | `UnixNanos`        | `int`             | Required         | Initialization timestamp in nanoseconds. |

*Note: Python constructors use `instrument_id`; Rust stores the same value as `id`.*

## Behavior

- `Cfd` has instrument class `Cfd`.
- It is never inverse and uses a multiplier of one.
- It has no activation timestamp, expiration timestamp, strike, or option kind.
- Use the source market type when a venue offers both cash instruments and CFDs.

## Example

<Tabs items={['Rust', 'Python']}>
<Tab value="Rust">

```rust
use nautilus_model::instruments::Cfd;

fn quote_currency(instrument: &Cfd) -> String {
    instrument.quote_currency.to_string()
}
```

</Tab>
<Tab value="Python">

```python
from nautilus_trader.model.instruments import Cfd


def quote_currency(instrument: Cfd) -> str:
    return str(instrument.quote_currency)
```

</Tab>
</Tabs>

## Adapters

Representative adapters that create or consume `Cfd` instruments include:

- [Interactive Brokers](../../integrations/ib.md) for CFD contracts.

## Related guides

- [Currency Pair](currency_pair.md) covers cash FX and crypto spot pairs.
- [Commodity](commodity.md) covers spot commodity instruments.
