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
        assert (
            five_min_bar.open == corresponding_1min_bars[0].open
        ), "5-min bar should open at first 1-min bar"
        assert (
            five_min_bar.close == corresponding_1min_bars[-1].close
        ), "5-min bar should close at last 1-min bar"
        assert five_min_bar.high == max(
            bar.high for bar in corresponding_1min_bars
        ), "5-min high should be == max of 1-min highs"
        assert five_min_bar.low == min(
            bar.low for bar in corresponding_1min_bars
        ), "5-min low should be == min of 1-min lows"

    # Cleanup
    engine.dispose()
