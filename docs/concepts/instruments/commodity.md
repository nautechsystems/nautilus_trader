# Commodity

`Commodity` represents a spot commodity market such as gold, silver, oil, or another
physical asset quoted in a currency. It models a spot market, not a dated futures
contract.

Examples include `XAUUSD.IDEALPRO` and venue-specific commodity cash symbols.

## Fields

| Field              | Rust type          | Python type       | Required/default | Notes                                      |
|--------------------|--------------------|-------------------|------------------|--------------------------------------------|
| `instrument_id`    | `InstrumentId`     | `InstrumentId`    | Required         | Stored as `id` in Rust.                    |
| `raw_symbol`       | `Symbol`           | `Symbol`          | Required         | Native venue symbol.                       |
| `asset_class`      | `AssetClass`       | `AssetClass`      | Required         | Commodity asset classification.            |
| `quote_currency`   | `Currency`         | `Currency`        | Required         | Currency used to price the commodity.      |
| `price_precision`  | `u8`               | `int`             | Required         | Decimal places allowed for prices.         |
| `size_precision`   | `u8`               | `int`             | Required         | Decimal places allowed for order sizes.    |
| `price_increment`  | `Price`            | `Price`           | Required         | Smallest valid price step.                 |
| `size_increment`   | `Quantity`         | `Quantity`        | Required         | Smallest valid size step.                  |
| `ts_event`         | `UnixNanos`        | `int`             | Required         | Event timestamp in nanoseconds.            |
| `ts_init`          | `UnixNanos`        | `int`             | Required         | Initialization timestamp in nanoseconds.   |
| `base_currency`    | N/A                | `Currency \| None` | `None`           | Python‑only base asset currency, if known. |
| `lot_size`         | `Option<Quantity>` | `Quantity \| None` | `None`           | Rounded lot or board size.                 |
| `max_quantity`     | `Option<Quantity>` | `Quantity \| None` | `None`           | Maximum order quantity.                    |
| `min_quantity`     | `Option<Quantity>` | `Quantity \| None` | `None`           | Minimum order quantity.                    |
| `max_notional`     | `Option<Money>`    | `Money \| None`    | `None`           | Maximum order notional value.              |
| `min_notional`     | `Option<Money>`    | `Money \| None`    | `None`           | Minimum order notional value.              |
| `max_price`        | `Option<Price>`    | `Price \| None`    | `None`           | Maximum valid quote or order price.        |
| `min_price`        | `Option<Price>`    | `Price \| None`    | `None`           | Minimum valid quote or order price.        |
| `margin_init`      | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Initial margin rate.                       |
| `margin_maint`     | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Maintenance margin rate.                   |
| `maker_fee`        | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Maker fee rate. Negative values rebate.    |
| `taker_fee`        | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Taker fee rate. Negative values rebate.    |
| `tick_scheme_name` | N/A                | `str \| None`      | `None`           | Registered variable tick scheme name.      |
| `info`             | `Option<Params>`   | `dict \| None`     | `None`           | Adapter metadata.                          |

*Note: Python constructors use `instrument_id`; Rust stores the same value as `id`.*

## Behavior

- `Commodity` has instrument class `Spot`.
- It is never inverse, and its cost currency is the quote currency.
- It has no activation timestamp, expiry, strike, option kind, or settlement currency field.
- Use `FuturesContract` for dated exchange-traded commodity futures.

## Example

<Tabs items={['Rust', 'Python']}>
<Tab value="Rust">

```rust
use nautilus_model::instruments::Commodity;

fn quote_currency(instrument: &Commodity) -> String {
    instrument.quote_currency.to_string()
}
```

</Tab>
<Tab value="Python">

```python
from nautilus_trader.model.instruments import Commodity


def quote_currency(instrument: Commodity) -> str:
    return str(instrument.quote_currency)
```

</Tab>
</Tabs>

## Adapters

Representative adapters that create or consume `Commodity` instruments include:

- [Interactive Brokers](../../integrations/ib.md) for spot commodity and metal contracts.

## Related guides

- [Futures Contract](futures_contract.md) covers dated futures on commodity underlyings.
- [Data](../data.md) explains market data that references instruments.
