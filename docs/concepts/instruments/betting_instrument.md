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

```rust tab="Rust"
use chrono::{TimeZone, Utc};
use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::{InstrumentId, Symbol},
    instruments::BettingInstrument,
    types::{Currency, Money, Price, Quantity},
};
use rust_decimal_macros::dec;
use ustr::Ustr;

let event_open = Utc.with_ymd_and_hms(2022, 2, 7, 23, 30, 0).unwrap();
let market_start = Utc.with_ymd_and_hms(2022, 2, 7, 23, 30, 0).unwrap();

let selection = BettingInstrument::new(
    InstrumentId::from("1-123456789.BETFAIR"),
    Symbol::from("1-123456789"),
    6423,
    Ustr::from("American Football"),
    12_282_733,
    Ustr::from("NFL"),
    29_678_534,
    Ustr::from("NFL"),
    Ustr::from("GB"),
    UnixNanos::from(event_open.timestamp_nanos_opt().unwrap() as u64),
    Ustr::from("ODDS"),
    Ustr::from("1-123456789"),
    Ustr::from("AFC Conference Winner"),
    Ustr::from("SPECIAL"),
    UnixNanos::from(market_start.timestamp_nanos_opt().unwrap() as u64),
    50214,
    Ustr::from("Kansas City Chiefs"),
    0.0,
    Currency::from("GBP"),
    2,
    2,
    Price::from("0.01"),
    Quantity::from("0.01"),
    Some(Quantity::from("1000")),
    Some(Quantity::from("1")),
    Some(Money::from("10000 GBP")),
    Some(Money::from("10 GBP")),
    Some(Price::from("100.00")),
    Some(Price::from("1.00")),
    Some(dec!(1)),
    Some(dec!(1)),
    Some(dec!(0)),
    Some(dec!(0)),
    None,
    UnixNanos::default(),
    UnixNanos::default(),
);
```

```python tab="Python"
import pandas as pd

from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.instruments import BettingInstrument
from nautilus_trader.model.objects import Money

selection = BettingInstrument(
    venue_name="BETFAIR",
    event_type_id=6423,
    event_type_name="American Football",
    competition_id=12282733,
    competition_name="NFL",
    event_id=29678534,
    event_name="NFL",
    event_country_code="GB",
    event_open_date=pd.Timestamp("2022-02-07 23:30:00+00:00"),
    betting_type="ODDS",
    market_id="1-123456789",
    market_name="AFC Conference Winner",
    market_type="SPECIAL",
    market_start_time=pd.Timestamp("2022-02-07 23:30:00+00:00"),
    selection_id=50214,
    selection_name="Kansas City Chiefs",
    currency="GBP",
    selection_handicap=0.0,
    price_precision=2,
    size_precision=2,
    min_notional=Money(1, GBP),
    ts_event=0,
    ts_init=0,
)
```

## Adapters

Representative adapters that create or consume `BettingInstrument` instruments include:

- [Betfair](../../integrations/betfair.md) for sports betting markets.
- [Betfair v2](../../integrations/betfair_v2.md) for sports betting markets.

## Related guides

- [Accounting](../accounting.md) covers betting account behavior.
- [Data](../data.md) explains market data that references instruments.
