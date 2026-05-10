# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import pytest

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.config import LoggingConfig
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarSpecification
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AggregationSource
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.persistence.wranglers import BarDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.trading.strategy import Strategy


USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")
SECONDS_NS = 1_000_000_000


def assert_monotonic(timestamps: list[int], context: str = "") -> None:
    """
    Assert that timestamps are monotonically non-decreasing.
    """
    assert len(timestamps) > 0, f"No timestamps recorded{f' ({context})' if context else ''}"
    for i in range(1, len(timestamps)):
        prev_ts, curr_ts = timestamps[i - 1], timestamps[i]
        assert curr_ts >= prev_ts, (
            f"Monotonicity violated at index {i}{f' ({context})' if context else ''}: "
            f"prev={prev_ts}, curr={curr_ts}, diff={curr_ts - prev_ts}ns"
        )


class SingleAlertStrategy(Strategy):
    """
    Strategy that schedules one time alert per bar at a configurable offset.
    """

    def __init__(self, alert_offset_ns: int):
        super().__init__()
        self.alert_offset_ns = alert_offset_ns
        self.timestamps: list[int] = []
        self.alert_counter = 0

    def on_start(self):
        bar_type = BarType(
            instrument_id=USDJPY_SIM.id,
            bar_spec=BarSpecification(1, BarAggregation.MINUTE, PriceType.BID),
            aggregation_source=AggregationSource.EXTERNAL,
        )
        self.subscribe_bars(bar_type)

    def on_bar(self, bar: Bar):
        self.timestamps.append(bar.ts_event)
        alert_time_ns = bar.ts_event + self.alert_offset_ns
        self.clock.set_time_alert_ns(
            name=f"alert_{self.alert_counter}",
            alert_time_ns=alert_time_ns,
            callback=self._on_alert,
        )
        self.alert_counter += 1

    def _on_alert(self, event):
        self.timestamps.append(event.ts_event)


class ChainedAlertStrategy(Strategy):
    """
    Strategy that schedules chained alerts (each alert schedules the next).
    """

    def __init__(self, chain_length: int, chain_interval_ns: int):
        super().__init__()
        self.chain_length = chain_length
        self.chain_interval_ns = chain_interval_ns
        self.timestamps: list[int] = []
        self.alert_counter = 0

    def on_start(self):
        bar_type = BarType(
            instrument_id=USDJPY_SIM.id,
            bar_spec=BarSpecification(1, BarAggregation.MINUTE, PriceType.BID),
            aggregation_source=AggregationSource.EXTERNAL,
        )
        self.subscribe_bars(bar_type)

    def on_bar(self, bar: Bar):
        self.timestamps.append(bar.ts_event)
        self._schedule_chain(bar.ts_event, self.chain_length)

    def _schedule_chain(self, base_time_ns: int, remaining: int):
        if remaining <= 0:
            return
        alert_time_ns = base_time_ns + self.chain_interval_ns
        self.clock.set_time_alert_ns(
            name=f"chain_{self.alert_counter}",
            alert_time_ns=alert_time_ns,
            callback=lambda event, r=remaining - 1: self._on_chain_alert(event, r),
        )
        self.alert_counter += 1

    def _on_chain_alert(self, event, remaining: int):
        self.timestamps.append(event.ts_event)
        self._schedule_chain(event.ts_event, remaining)


class MultipleAlertsStrategy(Strategy):
    """
    Strategy that schedules multiple alerts at different offsets per bar.
    """

    def __init__(self, alert_offsets_ns: list[int]):
        super().__init__()
        self.alert_offsets_ns = alert_offsets_ns
        self.timestamps: list[int] = []
        self.alert_counter = 0

    def on_start(self):
        bar_type = BarType(
            instrument_id=USDJPY_SIM.id,
            bar_spec=BarSpecification(1, BarAggregation.MINUTE, PriceType.BID),
            aggregation_source=AggregationSource.EXTERNAL,
        )
        self.subscribe_bars(bar_type)

    def on_bar(self, bar: Bar):
        self.timestamps.append(bar.ts_event)
        for offset in self.alert_offsets_ns:
            alert_time_ns = bar.ts_event + offset
            self.clock.set_time_alert_ns(
                name=f"alert_{self.alert_counter}",
                alert_time_ns=alert_time_ns,
                callback=self._on_alert,
            )
            self.alert_counter += 1

    def _on_alert(self, event):
        self.timestamps.append(event.ts_event)


class RepeatingTimerStrategy(Strategy):
    """
    Strategy that uses a repeating timer alongside bar processing.
    """

    def __init__(self, timer_interval_ns: int):
        super().__init__()
        self.timer_interval_ns = timer_interval_ns
        self.timestamps: list[int] = []

    def on_start(self):
        bar_type = BarType(
            instrument_id=USDJPY_SIM.id,
            bar_spec=BarSpecification(1, BarAggregation.MINUTE, PriceType.BID),
            aggregation_source=AggregationSource.EXTERNAL,
        )
        self.subscribe_bars(bar_type)
        self.clock.set_timer_ns(
            name="repeating",
            interval_ns=self.timer_interval_ns,
            start_time_ns=0,
            stop_time_ns=0,
            callback=self._on_timer,
        )

    def on_bar(self, bar: Bar):
        self.timestamps.append(bar.ts_event)

    def _on_timer(self, event):
        self.timestamps.append(event.ts_event)


@pytest.fixture
def engine():
    engine = BacktestEngine(
        BacktestEngineConfig(logging=LoggingConfig(bypass_logging=True)),
    )
    engine.add_venue(
        venue=Venue("SIM"),
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        starting_balances=[Money(1_000_000, USD)],
        fill_model=FillModel(),
    )
    engine.add_instrument(USDJPY_SIM)
    yield engine
    engine.reset()
    engine.dispose()


def load_bars(count: int = 20) -> list[Bar]:
    bar_type = BarType(
        instrument_id=USDJPY_SIM.id,
        bar_spec=BarSpecification(1, BarAggregation.MINUTE, PriceType.BID),
        aggregation_source=AggregationSource.EXTERNAL,
    )
    provider = TestDataProvider()
    wrangler = BarDataWrangler(bar_type, USDJPY_SIM)
    return wrangler.process(provider.read_csv_bars("fxcm/usdjpy-m1-bid-2013.csv")[:count])


@pytest.mark.parametrize(
    "alert_offset_seconds",
    [10, 30, 45, 59],
    ids=["10s", "30s", "45s", "59s"],
)
def test_single_alert_at_offset(engine: BacktestEngine, alert_offset_seconds: int):
    """
    Alert scheduled N seconds after bar fires before next 1-min bar.
    """
    # Arrange
    engine.add_data(load_bars(20))
    strategy = SingleAlertStrategy(alert_offset_ns=alert_offset_seconds * SECONDS_NS)
    engine.add_strategy(strategy)

    # Act
    engine.run()

    # Assert
    assert_monotonic(strategy.timestamps, f"offset={alert_offset_seconds}s")
    assert len(strategy.timestamps) >= 38


def test_chained_alerts_within_bar_interval(engine: BacktestEngine):
    """
    Chained alerts (each schedules the next) maintain monotonicity.
    """
    # Arrange
    engine.add_data(load_bars(10))
    strategy = ChainedAlertStrategy(chain_length=3, chain_interval_ns=15 * SECONDS_NS)
    engine.add_strategy(strategy)

    # Act
    engine.run()

    # Assert
    assert_monotonic(strategy.timestamps, "chained alerts")
    assert len(strategy.timestamps) >= 25


@pytest.mark.parametrize(
    "offsets_seconds",
    [
        [10, 20, 30],
        [15, 30, 45],
        [5, 25, 55],
    ],
    ids=["10-20-30", "15-30-45", "5-25-55"],
)
def test_multiple_alerts_per_bar(engine: BacktestEngine, offsets_seconds: list[int]):
    """
    Multiple alerts scheduled per bar fire in correct order.
    """
    # Arrange
    engine.add_data(load_bars(10))
    offsets_ns = [o * SECONDS_NS for o in offsets_seconds]
    strategy = MultipleAlertsStrategy(alert_offsets_ns=offsets_ns)
    engine.add_strategy(strategy)

    # Act
    engine.run()

    # Assert
    assert_monotonic(strategy.timestamps, f"offsets={offsets_seconds}")
    assert len(strategy.timestamps) >= 25


def test_alerts_at_same_timestamp(engine: BacktestEngine):
    """
    Multiple alerts scheduled for exact same timestamp.
    """
    # Arrange
    engine.add_data(load_bars(10))
    strategy = MultipleAlertsStrategy(
        alert_offsets_ns=[30 * SECONDS_NS, 30 * SECONDS_NS, 30 * SECONDS_NS],
    )
    engine.add_strategy(strategy)

    # Act
    engine.run()

    # Assert
    assert_monotonic(strategy.timestamps, "same timestamp alerts")


@pytest.mark.parametrize(
    "timer_interval_seconds",
    [20, 30, 45],
    ids=["20s", "30s", "45s"],
)
def test_repeating_timer(engine: BacktestEngine, timer_interval_seconds: int):
    """
    Repeating timer interleaved with bars maintains monotonicity.
    """
    # Arrange
    engine.add_data(load_bars(10))
    strategy = RepeatingTimerStrategy(timer_interval_ns=timer_interval_seconds * SECONDS_NS)
    engine.add_strategy(strategy)

    # Act
    engine.run()

    # Assert
    assert_monotonic(strategy.timestamps, f"timer interval={timer_interval_seconds}s")
    assert len(strategy.timestamps) > 10


def test_multiple_strategies_with_alerts(engine: BacktestEngine):
    """
    Multiple strategies each scheduling alerts maintain global monotonicity.
    """
    # Arrange
    engine.add_data(load_bars(10))
    strategy1 = SingleAlertStrategy(alert_offset_ns=20 * SECONDS_NS)
    strategy2 = SingleAlertStrategy(alert_offset_ns=40 * SECONDS_NS)
    engine.add_strategy(strategy1)
    engine.add_strategy(strategy2)

    # Act
    engine.run()

    # Assert
    assert_monotonic(strategy1.timestamps, "strategy1")
    assert_monotonic(strategy2.timestamps, "strategy2")
    combined = sorted(
        [(ts, "s1") for ts in strategy1.timestamps] + [(ts, "s2") for ts in strategy2.timestamps],
        key=lambda x: (x[0], x[1]),
    )
    combined_ts = [ts for ts, _ in combined]
    assert_monotonic(combined_ts, "combined strategies")


def test_alert_scheduled_at_exact_next_bar_time(engine: BacktestEngine):
    """
    Alert scheduled for exact same timestamp as next bar.
    """
    # Arrange
    engine.add_data(load_bars(10))
    strategy = SingleAlertStrategy(alert_offset_ns=60 * SECONDS_NS)
    engine.add_strategy(strategy)

    # Act
    engine.run()

    # Assert
    assert_monotonic(strategy.timestamps, "alert at bar time")


def test_deeply_chained_alerts(engine: BacktestEngine):
    """
    Deep chain of alerts (each callback schedules next) maintains monotonicity.
    """
    # Arrange
    engine.add_data(load_bars(5))
    strategy = ChainedAlertStrategy(chain_length=10, chain_interval_ns=5 * SECONDS_NS)
    engine.add_strategy(strategy)

    # Act
    engine.run()

    # Assert
    assert_monotonic(strategy.timestamps, "deep chain")
    assert len(strategy.timestamps) >= 30


class AlertAfterLastBarStrategy(Strategy):
    """
    Strategy that schedules an alert beyond the last bar timestamp.
    """

    def __init__(self, alert_offset_ns: int):
        super().__init__()
        self.alert_offset_ns = alert_offset_ns
        self.alert_timestamps: list[int] = []
        self.last_bar_ts: int = 0
        self.alert_counter = 0

    def on_start(self):
        bar_type = BarType(
            instrument_id=USDJPY_SIM.id,
            bar_spec=BarSpecification(1, BarAggregation.MINUTE, PriceType.BID),
            aggregation_source=AggregationSource.EXTERNAL,
        )
        self.subscribe_bars(bar_type)

    def on_bar(self, bar: Bar):
        self.last_bar_ts = bar.ts_event
        alert_time_ns = bar.ts_event + self.alert_offset_ns
        self.clock.set_time_alert_ns(
            name=f"future_alert_{self.alert_counter}",
            alert_time_ns=alert_time_ns,
            callback=self._on_alert,
        )
        self.alert_counter += 1

    def _on_alert(self, event):
        self.alert_timestamps.append(event.ts_event)


def test_alerts_at_or_before_last_timestamp_fire(engine: BacktestEngine):
    """
    Alerts scheduled at or before the last data timestamp fire correctly.

    Alerts scheduled beyond the last timestamp do not fire.

    """
    # Arrange
    bars = load_bars(5)
    engine.add_data(bars)

    # Alert 2 minutes after each bar - bars at 0,1,2,3,4 min, alerts at 2,3,4,5,6 min
    # Only alerts at or before last data (4 min) fire: alerts at 2,3,4 min = 3 alerts
    strategy = AlertAfterLastBarStrategy(alert_offset_ns=120 * SECONDS_NS)
    engine.add_strategy(strategy)

    # Act
    engine.run()

    # Assert - only 3 alerts fire (those at or before last data timestamp)
    assert len(strategy.alert_timestamps) == 3, (
        f"Expected 3 alerts, was {len(strategy.alert_timestamps)}"
    )
    assert_monotonic(strategy.alert_timestamps, "alerts at last timestamp")


class ChainedAlertAtLastTimestampStrategy(Strategy):
    """
    Strategy that schedules chained alerts AT the last bar timestamp.

    Tests that callbacks during the final flush can schedule new alerts as long as those
    alerts are at or before the last data timestamp.

    """

    def __init__(self):
        super().__init__()
        self.alert_timestamps: list[int] = []
        self.bar_count = 0
        self.last_bar_ts = 0

    def on_start(self):
        bar_type = BarType(
            instrument_id=USDJPY_SIM.id,
            bar_spec=BarSpecification(1, BarAggregation.MINUTE, PriceType.BID),
            aggregation_source=AggregationSource.EXTERNAL,
        )
        self.subscribe_bars(bar_type)

    def on_bar(self, bar: Bar):
        self.bar_count += 1
        self.last_bar_ts = bar.ts_event
        # On last bar (5th), schedule first alert AT the last bar timestamp
        if self.bar_count == 5:
            self.clock.set_time_alert_ns(
                name="chain_0",
                alert_time_ns=bar.ts_event,  # At last timestamp, not beyond
                callback=self._on_chain_alert,
            )

    def _on_chain_alert(self, event):
        self.alert_timestamps.append(event.ts_event)
        chain_num = len(self.alert_timestamps)
        if chain_num < 3:
            # Schedule next alert also at last timestamp (chained at same time)
            self.clock.set_time_alert_ns(
                name=f"chain_{chain_num}",
                alert_time_ns=self.last_bar_ts,  # Still at last timestamp
                callback=self._on_chain_alert,
            )


def test_chained_alerts_at_last_timestamp(engine: BacktestEngine):
    """
    Chained alerts scheduled at the last timestamp during flush all fire.
    """
    # Arrange
    engine.add_data(load_bars(5))
    strategy = ChainedAlertAtLastTimestampStrategy()
    engine.add_strategy(strategy)

    # Act
    engine.run()

    # Assert - all 3 chained alerts should fire (all at last timestamp)
    assert len(strategy.alert_timestamps) == 3, (
        f"Expected 3 chained alerts, was {len(strategy.alert_timestamps)}"
    )
    assert_monotonic(strategy.alert_timestamps, "chained alerts at last timestamp")
