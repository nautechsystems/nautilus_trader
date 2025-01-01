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

from nautilus_trader.indicators.average.sma import SimpleMovingAverage
from nautilus_trader.model.enums import PriceType
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestSimpleMovingAverage:
    def setup(self):
        # Fixture Setup
        self.sma = SimpleMovingAverage(10)

    def test_name_returns_expected_string(self):
        # Arrange, Act, Assert
        assert self.sma.name == "SimpleMovingAverage"

    def test_str_repr_returns_expected_string(self):
        # Arrange, Act, Assert
        assert str(self.sma) == "SimpleMovingAverage(10)"
        assert repr(self.sma) == "SimpleMovingAverage(10)"

    def test_period_returns_expected_value(self):
        # Arrange, Act, Assert
        assert self.sma.period == 10

    def test_initialized_without_inputs_returns_false(self):
        # Arrange, Act, Assert
        assert self.sma.initialized is False

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        self.sma.update_raw(1.0)
        self.sma.update_raw(2.0)
        self.sma.update_raw(3.0)
        self.sma.update_raw(4.0)
        self.sma.update_raw(5.0)
        self.sma.update_raw(6.0)
        self.sma.update_raw(7.0)
        self.sma.update_raw(8.0)
        self.sma.update_raw(9.0)
        self.sma.update_raw(10.0)

        # Act, Assert
        assert self.sma.initialized is True
        assert self.sma.count == 10
        assert self.sma.value == 5.5

    def test_handle_quote_tick_updates_indicator(self):
        # Arrange
        indicator = SimpleMovingAverage(10, PriceType.MID)

        tick = TestDataStubs.quote_tick()

        # Act
        indicator.handle_quote_tick(tick)

        # Assert
        assert indicator.has_inputs
        assert indicator.value == 1.0

    def test_handle_trade_tick_updates_indicator(self):
        # Arrange
        indicator = SimpleMovingAverage(10)

        tick = TestDataStubs.trade_tick()

        # Act
        indicator.handle_trade_tick(tick)

        # Assert
        assert indicator.has_inputs
        assert indicator.value == 1.0

    def test_handle_bar_updates_indicator(self):
        # Arrange
        indicator = SimpleMovingAverage(10)

        bar = TestDataStubs.bar_5decimal()

        # Act
        indicator.handle_bar(bar)

        # Assert
        assert indicator.has_inputs
        assert indicator.value == 1.00003

    def test_value_with_one_input_returns_expected_value(self):
        # Arrange
        self.sma.update_raw(1.0)

        # Act, Assert
        assert self.sma.value == 1.0

    def test_value_with_three_inputs_returns_expected_value(self):
        # Arrange
        self.sma.update_raw(1.0)
        self.sma.update_raw(2.0)
        self.sma.update_raw(3.0)

        # Act, Assert
        assert self.sma.value == 2.0

    def test_value_at_returns_expected_value(self):
        # Arrange
        self.sma.update_raw(1.0)
        self.sma.update_raw(2.0)
        self.sma.update_raw(3.0)

        # Act, Assert
        assert self.sma.value == 2.0

    def test_handle_quote_tick_updates_with_expected_value(self):
        # Arrange
        sma_for_ticks1 = SimpleMovingAverage(10, PriceType.ASK)
        sma_for_ticks2 = SimpleMovingAverage(10, PriceType.MID)
        sma_for_ticks3 = SimpleMovingAverage(10, PriceType.BID)

        tick = TestDataStubs.quote_tick(
            bid_price=1.00001,
            ask_price=1.00003,
        )

        # Act
        sma_for_ticks1.handle_quote_tick(tick)
        sma_for_ticks2.handle_quote_tick(tick)
        sma_for_ticks3.handle_quote_tick(tick)

        # Assert
        assert sma_for_ticks1.has_inputs
        assert sma_for_ticks2.has_inputs
        assert sma_for_ticks3.has_inputs
        assert sma_for_ticks1.value == 1.00003
        assert sma_for_ticks2.value == 1.00002
        assert sma_for_ticks3.value == 1.00001

    def test_handle_trade_tick_updates_with_expected_value(self):
        # Arrange
        sma_for_ticks = SimpleMovingAverage(10)

        tick = TestDataStubs.trade_tick()

        # Act
        sma_for_ticks.handle_trade_tick(tick)

        # Assert
        assert sma_for_ticks.has_inputs
        assert sma_for_ticks.value == 1.0

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        for _i in range(1000):
            self.sma.update_raw(1.0)

        # Act
        self.sma.reset()

        # Assert
        assert not self.sma.initialized
        assert self.sma.value == 0
