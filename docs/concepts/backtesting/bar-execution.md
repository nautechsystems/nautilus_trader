# Bar-Based Execution

Bar data records the open, high, low, close, and volume for an interval. It does not record when
each price occurred within that interval or whether the high preceded the low. Bar-based execution
therefore simulates a plausible intrabar path rather than reconstructing the original trades.

NautilusTrader converts each execution bar into synthetic market updates for an L1 order book.
Resting orders match as those updates move through the bar.

## Bar timestamp convention

:::warning
For execution simulation, each bar's initialization timestamp (`ts_init`) must represent the
**close** of the interval. This prevents the complete bar from becoming visible before it formed.
:::

The event timestamp (`ts_event`) may represent the open or close, depending on the data source:

- For bars timestamped at the close, set `ts_init` to the same timestamp.
- For bars timestamped at the open, set `ts_init = ts_event + interval_ns`. For example, add
  `60_000_000_000` nanoseconds for one-minute bars.

Where an adapter provides a setting such as `bars_timestamp_on_close=True`, prefer that setting so
the stored data uses the expected convention. For custom data, populate `ts_event` and `ts_init`
before constructing `Bar` objects, encoding Arrow record batches, writing a catalog, or calling
`add_data()`. The V2 `BarDataWrangler` consumes explicit timestamp fields and does not expose a
`ts_init_delta` argument. Verify the result on a small sample before running a backtest.

## Processing bar data

Bar execution applies only when:

- The venue has `bar_execution=True`.
- The venue uses `BookType.L1_MBP`.
- The bar has an external aggregation source.

Internally aggregated bars and bars sent to L2 or L3 venues still reach subscribed strategies, but
they do not update the matching engine's book or trigger matching.

For each applicable bar, the engine:

1. Selects the most granular configured bar type for the instrument.
1. Splits the bar volume across four synthetic updates.
1. Processes the open, high and low in the configured order, then the close.
1. Matches orders after each synthetic update.
1. Dispatches the complete bar to actors and strategies.

Orders already resting at the start of the bar can therefore fill at an intermediate OHLC point.
Orders submitted from `on_bar` arrive only after all four points for that bar have been processed.

## OHLC price simulation

The engine splits the bar volume evenly across the four price points. It assigns any remainder to
the close so the synthetic updates preserve total volume. If one quarter of the volume is below the
instrument's minimum `size_increment`, each point uses the minimum increment.

The venue's `bar_adaptive_high_low_ordering` option controls the intrabar path:

- With `False` (the default), every bar uses `Open -> High -> Low -> Close`.
- With `True`, the engine visits the extreme closest to the open first:
  - If the open is closer to the high, it uses `Open -> High -> Low -> Close`.
  - If the open is closer to the low, it uses `Open -> Low -> High -> Close`.

The adaptive path is a deterministic heuristic, not a reconstruction of the actual trade sequence.
Its accuracy depends on the market, interval, and data source. An
[exploratory EUR/USD analysis](https://gist.github.com/stefansimik/d387e1d9ff784a8973feca0cde51e363)
motivates the distance heuristic but does not establish a general accuracy rate.

The path matters when both a protective stop and a profit target lie inside the same bar because
the first visited level determines which order can fill first.

Configure adaptive ordering on the venue:

```python
from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.model import AccountType
from nautilus_trader.model import Money
from nautilus_trader.model import OmsType
from nautilus_trader.model import Venue

engine = BacktestEngine(BacktestEngineConfig())
engine.add_venue(
    venue=Venue("SIM"),
    oms_type=OmsType.NETTING,
    account_type=AccountType.CASH,
    starting_balances=[Money.from_str("10_000 USDT")],
    bar_adaptive_high_low_ordering=True,
)
```

## Order submission timing

Bar N's OHLC sequence runs before `on_bar(N)`. Without a latency model, an order submitted from
`on_bar` settles immediately against the book left at bar N's close.

A latency model delays the order's effective arrival. With bar-only data and no intervening timer
events, an order that arrives at the next data timestamp settles after the next bar's OHLC sweep,
so it sees that bar's close. Quote ticks, trade ticks, or timer-driven settlement can drain the
command earlier against the book state at that time.

```python
from nautilus_trader.execution import StaticLatencyModel

engine.add_venue(
    venue=Venue("SIM"),
    oms_type=OmsType.NETTING,
    account_type=AccountType.CASH,
    starting_balances=[Money.from_str("10_000 USDT")],
    latency_model=StaticLatencyModel(base_latency_nanos=1_000_000_000),
)
```

:::note
The engine does not provide a native next-bar-open fill mode. A strategy can form a signal from a
completed prior bar without look-ahead, but the next bar's open is processed before that next bar
is dispatched. Using the current bar's open from its `on_bar` callback would introduce look-ahead;
using latency with bar-only data normally settles against a later book state, not the next open.
:::

## Internal bar aggregation timing

When the data engine aggregates time bars from ticks, a timer closes each bar at the interval
boundary. Data with the exact same timestamp may otherwise be processed after that close timer.

Set `time_bars_build_delay` in `DataEngineConfig` to delay the timer:

```python
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.data import DataEngineConfig

config = BacktestEngineConfig(
    data_engine=DataEngineConfig(
        time_bars_build_delay=1,
    ),
)
```

The value is in microseconds. A small delay, such as one microsecond, lets boundary data arrive
before the bar closes. It affects only internally aggregated bars.
