# Betting Instrument

`BettingInstrument` represents one selection in a sports or gaming market. It carries
event, competition, market, and selection metadata so Nautilus can treat the selection
as an instrument with prices, sizes, limits, margins, and fees.

Examples include Betfair match-odds selections and handicap market selections.

## Fields

| Field                | Rust type          | Python type        | Required/default | Notes                                    |
|----------------------|--------------------|--------------------|------------------|------------------------------------------|
| `instrument_id`      | `InstrumentId`     | `InstrumentId`     | Required         | Stored as `id` in Rust.                  |
| `raw_symbol`         | `Symbol`           | `Symbol`           | Required         | Native or generated venue symbol.        |
| `event_type_id`      | `u64`              | `int`              | Required         | Event type identifier.                   |
| `event_type_name`    | `Ustr`             | `str`              | Required         | Event type name, such as a sport.        |
| `competition_id`     | `u64`              | `int`              | Required         | Competition identifier.                  |
| `competition_name`   | `Ustr`             | `str`              | Required         | Competition name.                        |
| `event_id`           | `u64`              | `int`              | Required         | Event identifier.                        |
| `event_name`         | `Ustr`             | `str`              | Required         | Event name.                              |
| `event_country_code` | `Ustr`             | `str`              | Required         | Event country code.                      |
| `event_open_date`    | `UnixNanos`        | `int`              | Required         | Event open time.                         |
| `betting_type`       | `Ustr`             | `str`              | Required         | Betting type published by the venue.     |
| `market_id`          | `Ustr`             | `str`              | Required         | Market identifier.                       |
| `market_name`        | `Ustr`             | `str`              | Required         | Market name.                             |
| `market_type`        | `Ustr`             | `str`              | Required         | Market type, such as match odds.         |
| `market_start_time`  | `UnixNanos`        | `int`              | Required         | Market start time.                       |
| `selection_id`       | `u64`              | `int`              | Required         | Selection or runner identifier.          |
| `selection_name`     | `Ustr`             | `str`              | Required         | Selection or runner name.                |
| `selection_handicap` | `f64`              | `float`            | Required         | Handicap value for handicap markets.     |
| `currency`           | `Currency`         | `Currency`         | Required         | Quote and settlement currency.           |
| `price_precision`    | `u8`               | `int`              | Required         | Decimal places allowed for prices.       |
| `size_precision`     | `u8`               | `int`              | Required         | Decimal places allowed for order sizes.  |
| `price_increment`    | `Price`            | `Price`            | Required         | Price step, often set by a tick scheme.  |
| `size_increment`     | `Quantity`         | `Quantity`         | Required         | Minimum size step.                       |
| `max_quantity`       | `Option<Quantity>` | `Quantity \| None` | `None`           | Maximum order quantity.                  |
| `min_quantity`       | `Option<Quantity>` | `Quantity \| None` | `None`           | Minimum order quantity.                  |
| `max_notional`       | `Option<Money>`    | `Money \| None`    | `None`           | Maximum order notional value.            |
| `min_notional`       | `Option<Money>`    | `Money \| None`    | `None`           | Minimum order notional value.            |
| `max_price`          | `Option<Price>`    | `Price \| None`    | `None`           | Maximum valid quote or order price.      |
| `min_price`          | `Option<Price>`    | `Price \| None`    | `None`           | Minimum valid quote or order price.      |
| `margin_init`        | `Option<Decimal>`  | `Decimal \| None`  | `1`              | Initial margin rate.                     |
| `margin_maint`       | `Option<Decimal>`  | `Decimal \| None`  | `1`              | Maintenance margin rate.                 |
| `maker_fee`          | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Maker fee rate. Negative values rebate.  |
| `taker_fee`          | `Option<Decimal>`  | `Decimal \| None`  | `0`              | Taker fee rate. Negative values rebate.  |
| `tick_scheme`        | `Option<Ustr>`     | `str \| None`      | `None`           | Registered variable tick scheme name.    |
| `info`               | `Option<Params>`   | `dict \| None`     | `None`           | Adapter metadata.                        |
| `ts_event`           | `UnixNanos`        | `int`              | Required         | Event timestamp in nanoseconds.          |
| `ts_init`            | `UnixNanos`        | `int`              | Required         | Initialization timestamp in nanoseconds. |

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

let selection = BettingInstrument::builder()
    .instrument_id(InstrumentId::from("1-123456789.BETFAIR"))
    .raw_symbol(Symbol::from("1-123456789"))
    .event_type_id(6423)
    .event_type_name(Ustr::from("American Football"))
    .competition_id(12_282_733)
    .competition_name(Ustr::from("NFL"))
    .event_id(29_678_534)
    .event_name(Ustr::from("NFL"))
    .event_country_code(Ustr::from("GB"))
    .event_open_date(UnixNanos::from(event_open.timestamp_nanos_opt().unwrap() as u64))
    .betting_type(Ustr::from("ODDS"))
    .market_id(Ustr::from("1-123456789"))
    .market_name(Ustr::from("AFC Conference Winner"))
    .market_type(Ustr::from("SPECIAL"))
    .market_start_time(UnixNanos::from(market_start.timestamp_nanos_opt().unwrap() as u64))
    .selection_id(50214)
    .selection_name(Ustr::from("Kansas City Chiefs"))
    .selection_handicap(0.0)
    .currency(Currency::from("GBP"))
    .price_precision(2)
    .size_precision(2)
    .price_increment(Price::from("0.01"))
    .size_increment(Quantity::from("0.01"))
    .max_quantity(Quantity::from("1000"))
    .min_quantity(Quantity::from("1"))
    .max_notional(Money::from("10000 GBP"))
    .min_notional(Money::from("10 GBP"))
    .max_price(Price::from("100.00"))
    .min_price(Price::from("1.00"))
    .margin_init(dec!(1))
    .margin_maint(dec!(1))
    .maker_fee(dec!(0))
    .taker_fee(dec!(0))
    .ts_event(UnixNanos::default())
    .ts_init(UnixNanos::default())
    .build()
    .unwrap();
```

```python tab="Python"
import pandas as pd

from nautilus_trader.model import BettingInstrument
from nautilus_trader.model import Currency
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Money
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import Symbol
from nautilus_trader.model import Venue

GBP = Currency.from_str("GBP")

selection = BettingInstrument(
    instrument_id=InstrumentId(Symbol("1-123456789-50214"), Venue("BETFAIR")),
    raw_symbol=Symbol("1-123456789-50214"),
    event_type_id=6423,
    event_type_name="American Football",
    competition_id=12282733,
    competition_name="NFL",
    event_id=29678534,
    event_name="NFL",
    event_country_code="GB",
    event_open_date=pd.Timestamp("2022-02-07 23:30:00+00:00").value,
    betting_type="ODDS",
    market_id="1-123456789",
    market_name="AFC Conference Winner",
    market_type="SPECIAL",
    market_start_time=pd.Timestamp("2022-02-07 23:30:00+00:00").value,
    selection_id=50214,
    selection_name="Kansas City Chiefs",
    selection_handicap=0.0,
    currency=GBP,
    price_precision=2,
    size_precision=2,
    price_increment=Price.from_str("0.01"),
    size_increment=Quantity.from_str("0.01"),
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
