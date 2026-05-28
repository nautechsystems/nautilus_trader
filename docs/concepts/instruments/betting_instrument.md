# Betting Instrument

`BettingInstrument` represents one selection in a sports or gaming market. It carries
event, competition, market, and selection metadata so Nautilus can treat the selection
as an instrument with prices, sizes, limits, margins, and fees.

Examples include Betfair match-odds selections and handicap market selections.

## Fields

| Field                  | Rust type          | Python type       | Required/default | Notes                                      |
|------------------------|--------------------|-------------------|------------------|--------------------------------------------|
| `instrument_id`        | `InstrumentId`     | N/A               | Rust only        | Stored as `id` in Rust.                    |
| `raw_symbol`           | `Symbol`           | N/A               | Rust only        | Native or generated venue symbol.          |
| `venue_name`           | N/A                | `str`             | Python only      | Venue used to construct the instrument ID. |
| `event_type_id`        | `u64`              | `int`             | Required         | Event type identifier.                     |
| `event_type_name`      | `Ustr`             | `str`             | Required         | Event type name, such as a sport.          |
| `competition_id`       | `u64`              | `int`             | Required         | Competition identifier.                    |
| `competition_name`     | `Ustr`             | `str`             | Required         | Competition name.                          |
| `event_id`             | `u64`              | `int`             | Required         | Event identifier.                          |
| `event_name`           | `Ustr`             | `str`             | Required         | Event name.                                |
| `event_country_code`   | `Ustr`             | `str`             | Required         | Event country code.                        |
| `event_open_date`      | `UnixNanos`        | `datetime`        | Required         | Event open time.                           |
| `betting_type`         | `Ustr`             | `str`             | Required         | Betting type published by the venue.       |
| `market_id`            | `Ustr`             | `str`             | Required         | Market identifier.                         |
| `market_name`          | `Ustr`             | `str`             | Required         | Market name.                               |
| `market_type`          | `Ustr`             | `str`             | Required         | Market type, such as match odds.           |
| `market_start_time`    | `UnixNanos`        | `datetime`        | Required         | Market start time.                         |
| `selection_id`         | `u64`              | `int`             | Required         | Selection or runner identifier.            |
| `selection_name`       | `Ustr`             | `str`             | Required         | Selection or runner name.                  |
| `selection_handicap`   | `f64`              | `float`           | Required         | Handicap value for handicap markets.       |
| `currency`             | `Currency`         | `str`             | Required         | Quote and settlement currency.             |
| `price_precision`      | `u8`               | `int`             | Required         | Decimal places allowed for prices.         |
| `size_precision`       | `u8`               | `int`             | Required         | Decimal places allowed for order sizes.    |
| `price_increment`      | `Price`            | `Price \| None`    | Required/Rust    | Price step, often set by a tick scheme.    |
| `size_increment`       | `Quantity`         | `Quantity`        | Required/Rust    | Minimum size step.                         |
| `max_quantity`         | `Option<Quantity>` | `Quantity \| None` | `None`           | Maximum order quantity.                    |
| `min_quantity`         | `Option<Quantity>` | `Quantity \| None` | `None`           | Minimum order quantity.                    |
| `max_notional`         | `Option<Money>`    | `Money \| None`    | `None`           | Maximum order notional value.              |
| `min_notional`         | `Option<Money>`    | `Money \| None`    | `None`           | Minimum order notional value.              |
| `max_price`            | `Option<Price>`    | `Price \| None`    | `None`           | Maximum valid quote or order price.        |
| `min_price`            | `Option<Price>`    | `Price \| None`    | `None`           | Minimum valid quote or order price.        |
| `margin_init`          | `Option<Decimal>`  | `Decimal \| None`  | `1`              | Initial margin rate.                       |
| `margin_maint`         | `Option<Decimal>`  | `Decimal \| None`  | `1`              | Maintenance margin rate.                   |
| `maker_fee`            | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Maker fee rate. Negative values rebate.    |
| `taker_fee`            | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Taker fee rate. Negative values rebate.    |
| `tick_scheme_name`     | N/A                | `str \| None`      | `None`           | Registered variable tick scheme name.      |
| `info`                 | `Option<Params>`   | `dict \| None`     | `{}`/`None`      | Adapter metadata.                          |
| `ts_event`             | `UnixNanos`        | `int`             | Required         | Event timestamp in nanoseconds.            |
| `ts_init`              | `UnixNanos`        | `int`             | Required         | Initialization timestamp in nanoseconds.   |

*Note: Python builds the instrument ID and raw symbol from the venue, market, selection,
and handicap fields. Rust receives them as `instrument_id` and `raw_symbol`.*

## Behavior

- `BettingInstrument` has asset class `Alternative` and instrument class `SportsBetting`.
- Each selection or runner is modeled as its own instrument.
- Betting instruments commonly use a registered tick scheme for valid odds steps.
- Margin defaults to one because staking a bet typically reserves the full stake.

## Example

<Tabs items={['Rust', 'Python']}>
<Tab value="Rust">

```rust
use nautilus_model::instruments::BettingInstrument;

fn market_selection(instrument: &BettingInstrument) -> String {
    format!("{}: {}", instrument.market_name, instrument.selection_name)
}
```

</Tab>
<Tab value="Python">

```python
from nautilus_trader.model.instruments import BettingInstrument


def market_selection(instrument: BettingInstrument) -> str:
    return f"{instrument.market_name}: {instrument.selection_name}"
```

</Tab>
</Tabs>

## Adapters

Representative adapters that create or consume `BettingInstrument` instruments include:

- [Betfair](../../integrations/betfair.md) for sports betting markets.
- [Betfair v2](../../integrations/betfair_v2.md) for sports betting markets.

## Related guides

- [Accounting](../accounting.md) covers betting account behavior.
- [Data](../data.md) explains market data that references instruments.
