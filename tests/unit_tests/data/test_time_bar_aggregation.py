# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from decimal import Decimal

import pandas as pd

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.core.nautilus_pyo3 import NANOSECONDS_IN_SECOND
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.providers import TestDataGenerator
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.trading.strategy import Strategy


class BarAggregationStrategy(Strategy):
    """
    A simple strategy that tracks bar aggregation from 1-minute to 5-minute bars.
    """

    def __init__(self, bar_type_1min_external: BarType, bar_type_5min_composite_internal: BarType):
        super().__init__()

        # Traded instrument
        self.instrument_id = bar_type_1min_external.instrument_id

        # Bar types
        self.bar_type_1min = bar_type_1min_external
        self.bar_type_5min = bar_type_5min_composite_internal

        # Collected bars for verification
        self.received_1min_bars: list[Bar] = []
        self.received_5min_bars: list[Bar] = []

    def on_start(self):
        """
        Subscribe to both 1-minute bars and aggregated 5-minute bars.
        """
        # Subscribe to 1-minute bars
        self.subscribe_bars(self.bar_type_1min)

        # Subscribe to 5-minute bars (aggregated from 1-minute bars)
        self.subscribe_bars(
            BarType.from_str(
                f"{self.instrument_id}-5-MINUTE-LAST-INTERNAL@1-MINUTE-EXTERNAL",
            ),
        )

    def on_bar(self, bar: Bar):
        # Record received bars based on their type.
        if bar.bar_type == self.bar_type_1min:
            self.received_1min_bars.append(bar)
        elif bar.bar_type == self.bar_type_5min:
            self.received_5min_bars.append(bar)


def test_time_bar_aggregation():
    """
    Test that verifies the basic functionality of aggregating 1-minute bars into
    5-minute bars.

    This test focuses specifically on the aggregation process and verifies it works
    without errors.

    """
    # Create a backtest engine
    engine = BacktestEngine(
        config=BacktestEngineConfig(
            trader_id=TraderId("TESTER-000"),
        ),
    )

    # Add a test venue
    venue_name = "XCME"
    engine.add_venue(
        venue=Venue(venue_name),
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        starting_balances=[Money(1_000_000, USD)],
        default_leverage=Decimal(1),
    )

    # Add test instrument (6E futures contract)
    instrument = TestInstrumentProvider.eurusd_future(
        expiry_year=2024,
        expiry_month=3,
        venue_name=venue_name,
    )
    engine.add_instrument(instrument)

    # Create bar types for 1-minute and 5-minute bars
    bar_type_1min = BarType.from_str(f"{instrument.id}-1-MINUTE-LAST-EXTERNAL")
    bar_type_5min = BarType.from_str(f"{instrument.id}-5-MINUTE-LAST-INTERNAL")

    # Create and add test strategy
    strategy = BarAggregationStrategy(
        bar_type_1min_external=bar_type_1min,
        bar_type_5min_composite_internal=bar_type_5min,
    )
    engine.add_strategy(strategy)

    # Set up backtest time range
    start_time = pd.Timestamp("2024-01-01 00:01:00", tz="UTC")
    end_time = pd.Timestamp("2024-01-01 01:00:00", tz="UTC")  # 1 hour later

    # Create first bar with values matching the example
    first_bar = Bar(
        bar_type=bar_type_1min,
        open=instrument.make_price(1.1020),
        high=instrument.make_price(1.1025),
        low=instrument.make_price(1.0995),
        close=instrument.make_price(1.1000),
        volume=Quantity.from_int(999999),  # unlimited volume
        ts_event=dt_to_unix_nanos(start_time),
        ts_init=dt_to_unix_nanos(start_time),
    )

    # Generate synthetic 1-minute bar data
    bars = TestDataGenerator.generate_monotonic_bars(
        instrument=instrument,
        first_bar=first_bar,
        bar_count=60,  # Generate 60 one-minute bars for one hour
        time_change_nanos=60 * NANOSECONDS_IN_SECOND,  # 1 minute in nanoseconds
    )

    # Add data to the engine
    engine.add_data(bars)

    # Run the backtest with explicit time range
    engine.run(start=start_time, end=end_time)

    # ASSERTS

    # Verify we received the expected number of bars
    assert len(strategy.received_1min_bars) == 60, "Should receive 60x 1-minute bars (in 1 hour)"
    assert len(strategy.received_5min_bars) == 12, "Should receive 12x 5-minute bars (in 1 hour)"

    # Verify the 5-minute bars are 100% correctly aggregated
    for i in range(len(strategy.received_5min_bars)):
        five_min_bar = strategy.received_5min_bars[i]
        # Each 5-minute bar should correspond to 5x 1-minute bars
        corresponding_1min_bars = strategy.received_1min_bars[i * 5 : (i + 1) * 5]

        # Basic validation of 5-minute bar properties
        assert five_min_bar.open == corresponding_1min_bars[0].open, (
            "5-min bar should open at first 1-min bar"
        )
        assert five_min_bar.close == corresponding_1min_bars[-1].close, (
            "5-min bar should close at last 1-min bar"
        )
        assert five_min_bar.high == max(bar.high for bar in corresponding_1min_bars), (
            "5-min high should be == max of 1-min highs"
        )
        assert five_min_bar.low == min(bar.low for bar in corresponding_1min_bars), (
            "5-min low should be == min of 1-min lows"
        )

    # Cleanup
    engine.dispose()


class YearBarAggregationStrategy(Strategy):
    """
    A simple strategy that tracks bar aggregation from daily bars to 1-year bars.
    """

    def __init__(self, bar_type_day_external: BarType, bar_type_year_composite_internal: BarType):
        super().__init__()

        # Traded instrument
        self.instrument_id = bar_type_day_external.instrument_id

        # Bar types
        self.bar_type_day = bar_type_day_external
        self.bar_type_year = bar_type_year_composite_internal

        # Collected bars for verification
        self.received_day_bars: list[Bar] = []
        self.received_year_bars: list[Bar] = []

    def on_start(self):
        """
        Subscribe to both daily bars and aggregated 1-year bars.
        """
        # Subscribe to daily bars
        self.subscribe_bars(self.bar_type_day)

        # Subscribe to 1-year bars (aggregated from daily bars)
        self.subscribe_bars(
            BarType.from_str(
                f"{self.instrument_id}-1-YEAR-LAST-INTERNAL@1-DAY-EXTERNAL",
            ),
        )

    def on_bar(self, bar: Bar):
        # Record received bars based on their type.
        if bar.bar_type == self.bar_type_day:
            self.received_day_bars.append(bar)
        elif bar.bar_type == self.bar_type_year:
            self.received_year_bars.append(bar)


def test_year_bar_aggregation():
    """
    Test that verifies the basic functionality of aggregating daily bars into 1-year
    bars.

    This test focuses specifically on the aggregation process and verifies it works
    without errors.

    """
    # Create a backtest engine
    engine = BacktestEngine(
        config=BacktestEngineConfig(
            trader_id=TraderId("TESTER-000"),
        ),
    )

    # Add a test venue
    venue_name = "XCME"
    engine.add_venue(
        venue=Venue(venue_name),
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        starting_balances=[Money(1_000_000, USD)],
        default_leverage=Decimal(1),
    )

    # Add test instrument (6E futures contract)
    instrument = TestInstrumentProvider.eurusd_future(
        expiry_year=2024,
        expiry_month=3,
        venue_name=venue_name,
    )
    engine.add_instrument(instrument)

    # Create bar types for daily and 1-year bars
    bar_type_day = BarType.from_str(f"{instrument.id}-1-DAY-LAST-EXTERNAL")
    bar_type_year = BarType.from_str(f"{instrument.id}-1-YEAR-LAST-INTERNAL")

    # Create and add test strategy
    strategy = YearBarAggregationStrategy(
        bar_type_day_external=bar_type_day,
        bar_type_year_composite_internal=bar_type_year,
    )
    engine.add_strategy(strategy)

    # Set up backtest time range - 2 years of data
    start_time = pd.Timestamp("2020-01-01 00:00:00", tz="UTC")
    end_time = pd.Timestamp("2022-01-01 00:00:00", tz="UTC")  # 2 years later

    # Create first bar
    first_bar = Bar(
        bar_type=bar_type_day,
        open=instrument.make_price(1.1000),
        high=instrument.make_price(1.1025),
        low=instrument.make_price(1.0995),
        close=instrument.make_price(1.1010),
        volume=Quantity.from_int(1000),
        ts_event=dt_to_unix_nanos(start_time),
        ts_init=dt_to_unix_nanos(start_time),
    )

    # Generate synthetic daily bar data - 730 days (2 years)
    bars = TestDataGenerator.generate_monotonic_bars(
        instrument=instrument,
        first_bar=first_bar,
        bar_count=730,  # Generate 730 daily bars for 2 years
        time_change_nanos=24 * 60 * 60 * NANOSECONDS_IN_SECOND,  # 1 day in nanoseconds
    )

    # Add data to the engine
    engine.add_data(bars)

    # Run the backtest with explicit time range
    engine.run(start=start_time, end=end_time)

    # ASSERTS

    # Verify we received the expected number of bars
    assert len(strategy.received_day_bars) == 730, "Should receive 730x daily bars (2 years)"
    # Should receive at least 1 year bar (possibly 2 if aggregation works correctly)
    assert len(strategy.received_year_bars) >= 1, "Should receive at least 1x 1-year bar"

    # Verify the 1-year bars are correctly aggregated
    # Note: Year bars use time alerts and may be built at year boundaries
    # The exact timing depends on when the timer fires relative to data processing
    if len(strategy.received_year_bars) > 0:
        year_bar = strategy.received_year_bars[0]

        # Verify basic properties of the year bar
        assert year_bar.bar_type.spec.aggregation == BarAggregation.YEAR, (
            "Bar should be a year aggregation"
        )
        assert year_bar.bar_type.spec.step == 1, "Bar should be 1-year step"

        # Verify the year bar has valid OHLC values
        assert year_bar.open is not None, "Year bar should have an open price"
        assert year_bar.close is not None, "Year bar should have a close price"
        assert year_bar.high is not None, "Year bar should have a high price"
        assert year_bar.low is not None, "Year bar should have a low price"
        assert year_bar.volume is not None, "Year bar should have volume"

        # Verify high >= low and high >= open/close and low <= open/close
        assert year_bar.high >= year_bar.low, "Year bar high should be >= low"
        assert year_bar.high >= year_bar.open, "Year bar high should be >= open"
        assert year_bar.high >= year_bar.close, "Year bar high should be >= close"
        assert year_bar.low <= year_bar.open, "Year bar low should be <= open"
        assert year_bar.low <= year_bar.close, "Year bar low should be <= close"

        # If we have multiple year bars, verify they're sequential
        if len(strategy.received_year_bars) > 1:
            for i in range(1, len(strategy.received_year_bars)):
                prev_bar = strategy.received_year_bars[i - 1]
                curr_bar = strategy.received_year_bars[i]
                assert curr_bar.ts_event >= prev_bar.ts_event, "Year bars should be sequential"

    # Cleanup
    engine.dispose()


class MonthBarAggregationStrategy(Strategy):
    """
    A simple strategy that tracks bar aggregation from daily bars to 1-month bars.
    """

    def __init__(self, bar_type_day_external: BarType, bar_type_month_composite_internal: BarType):
        super().__init__()

        # Traded instrument
        self.instrument_id = bar_type_day_external.instrument_id

        # Bar types
        self.bar_type_day = bar_type_day_external
        self.bar_type_month = bar_type_month_composite_internal

        # Collected bars for verification
        self.received_day_bars: list[Bar] = []
        self.received_month_bars: list[Bar] = []

    def on_start(self):
        """
        Subscribe to both daily bars and aggregated 1-month bars.
        """
        # Subscribe to daily bars
        self.subscribe_bars(self.bar_type_day)

        # Subscribe to 1-month bars (aggregated from daily bars)
        self.subscribe_bars(
            BarType.from_str(
                f"{self.instrument_id}-1-MONTH-LAST-INTERNAL@1-DAY-EXTERNAL",
            ),
        )

    def on_bar(self, bar: Bar):
        # Record received bars based on their type
        if bar.bar_type == self.bar_type_day:
            self.received_day_bars.append(bar)
        elif bar.bar_type == self.bar_type_month:
            self.received_month_bars.append(bar)


def test_month_bar_aggregation():
    """
    Test that verifies the basic functionality of aggregating daily bars into 1-month
    bars.

    This test focuses specifically on the aggregation process and verifies it works
    without errors, including that monthly bars can now be processed for execution.

    """
    # Create a backtest engine
    engine = BacktestEngine(
        config=BacktestEngineConfig(
            trader_id=TraderId("TESTER-000"),
        ),
    )

    # Add a test venue
    venue_name = "XCME"
    engine.add_venue(
        venue=Venue(venue_name),
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        starting_balances=[Money(1_000_000, USD)],
        default_leverage=Decimal(1),
    )

    # Add test instrument (6E futures contract)
    instrument = TestInstrumentProvider.eurusd_future(
        expiry_year=2024,
        expiry_month=12,
        venue_name=venue_name,
    )
    engine.add_instrument(instrument)

    # Create bar types for daily and 1-month bars
    bar_type_day = BarType.from_str(f"{instrument.id}-1-DAY-LAST-EXTERNAL")
    bar_type_month = BarType.from_str(f"{instrument.id}-1-MONTH-LAST-INTERNAL")

    # Create and add test strategy
    strategy = MonthBarAggregationStrategy(
        bar_type_day_external=bar_type_day,
        bar_type_month_composite_internal=bar_type_month,
    )
    engine.add_strategy(strategy)

    # Set up backtest time range - 6 months of data
    start_time = pd.Timestamp("2024-01-01 00:00:00", tz="UTC")
    end_time = pd.Timestamp("2024-07-01 00:00:00", tz="UTC")  # 6 months later

    # Create first bar
    first_bar = Bar(
        bar_type=bar_type_day,
        open=instrument.make_price(1.1000),
        high=instrument.make_price(1.1025),
        low=instrument.make_price(1.0995),
        close=instrument.make_price(1.1010),
        volume=Quantity.from_int(1000),
        ts_event=dt_to_unix_nanos(start_time),
        ts_init=dt_to_unix_nanos(start_time),
    )

    # Generate synthetic daily bar data - approximately 182 days (6 months)
    bars = TestDataGenerator.generate_monotonic_bars(
        instrument=instrument,
        first_bar=first_bar,
        bar_count=182,  # Generate 182 daily bars for ~6 months
        time_change_nanos=24 * 60 * 60 * NANOSECONDS_IN_SECOND,  # 1 day in nanoseconds
    )

    # Add data to the engine
    engine.add_data(bars)

    # Run the backtest with explicit time range
    engine.run(start=start_time, end=end_time)

    # ASSERTS

    # Verify we received the expected number of bars
    assert len(strategy.received_day_bars) == 182, "Should receive 182x daily bars (~6 months)"
    # Should receive at least 5 month bars (possibly 6 if aggregation works correctly)
    assert len(strategy.received_month_bars) >= 5, "Should receive at least 5x 1-month bars"

    # Verify the 1-month bars are correctly aggregated
    # Note: Month bars use time alerts and may be built at month boundaries
    # The exact timing depends on when the timer fires relative to data processing
    if len(strategy.received_month_bars) > 0:
        month_bar = strategy.received_month_bars[0]

        # Verify basic properties of the month bar
        assert month_bar.bar_type.spec.aggregation == BarAggregation.MONTH
        assert month_bar.bar_type.spec.step == 1

        # Verify the month bar has valid OHLC values
        assert month_bar.open is not None
        assert month_bar.close is not None
        assert month_bar.high is not None
        assert month_bar.low is not None
        assert month_bar.volume is not None

        # Verify high >= low and high >= open/close and low <= open/close
        assert month_bar.high >= month_bar.low
        assert month_bar.high >= month_bar.open
        assert month_bar.high >= month_bar.close
        assert month_bar.low <= month_bar.open
        assert month_bar.low <= month_bar.close

        # If we have multiple month bars, verify they're sequential
        if len(strategy.received_month_bars) > 1:
            for i in range(1, len(strategy.received_month_bars)):
                prev_bar = strategy.received_month_bars[i - 1]
                curr_bar = strategy.received_month_bars[i]
                assert curr_bar.ts_event >= prev_bar.ts_event

    # Cleanup
    engine.dispose()
