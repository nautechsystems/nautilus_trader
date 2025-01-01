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

from nautilus_trader.indicators.average.hma import HullMovingAverage
from nautilus_trader.model.enums import PriceType
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestHullMovingAverage:
    def setup(self):
        # Fixture Setup
        self.hma = HullMovingAverage(10)

    def test_name_returns_expected_string(self):
        # Act, Assert
        assert self.hma.name == "HullMovingAverage"

    def test_str_repr_returns_expected_string(self):
        # Act, Assert
        assert str(self.hma) == "HullMovingAverage(10)"
        assert repr(self.hma) == "HullMovingAverage(10)"

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        self.hma.update_raw(1.00000)
        self.hma.update_raw(2.00000)
        self.hma.update_raw(3.00000)
        self.hma.update_raw(4.00000)
        self.hma.update_raw(5.00000)
        self.hma.update_raw(6.00000)
        self.hma.update_raw(7.00000)
        self.hma.update_raw(8.00000)
        self.hma.update_raw(9.00000)
        self.hma.update_raw(10.00000)

        # Act, Assert
        assert self.hma.initialized is True

    def test_handle_quote_tick_updates_indicator(self):
        # Arrange
        indicator = HullMovingAverage(10, PriceType.MID)

        tick = TestDataStubs.quote_tick()

        # Act
        indicator.handle_quote_tick(tick)

        # Assert
        assert indicator.has_inputs
        assert indicator.value == 1.0

    def test_handle_trade_tick_updates_indicator(self):
        # Arrange
        indicator = HullMovingAverage(10, PriceType.MID)

        tick = TestDataStubs.trade_tick()

        # Act
        indicator.handle_trade_tick(tick)

        # Assert
        assert indicator.has_inputs
        assert indicator.value == 1.0

    def test_handle_bar_updates_indicator(self):
        # Arrange
        indicator = HullMovingAverage(10)

        bar = TestDataStubs.bar_5decimal()

        # Act
        indicator.handle_bar(bar)

        # Assert
        assert indicator.has_inputs
        assert indicator.value == 1.00003

    def test_value_with_one_input_returns_expected_value(self):
        # Arrange
        self.hma.update_raw(1.00000)

        # Act, Assert
        assert self.hma.value == 1.0

    def test_value_with_three_inputs_returns_expected_value(self):
        # Arrange
        self.hma.update_raw(1.00000)
        self.hma.update_raw(2.00000)
        self.hma.update_raw(3.00000)

        # Act, Assert
        assert self.hma.value == 1.8245614035087718

    def test_value_with_ten_inputs_returns_expected_value(self):
        # Arrange
        self.hma.update_raw(1.00000)
        self.hma.update_raw(1.00010)
        self.hma.update_raw(1.00020)
        self.hma.update_raw(1.00030)
        self.hma.update_raw(1.00040)
        self.hma.update_raw(1.00050)
        self.hma.update_raw(1.00040)
        self.hma.update_raw(1.00030)
        self.hma.update_raw(1.00020)
        self.hma.update_raw(1.00010)
        self.hma.update_raw(1.00000)

        # Act, Assert
        assert self.hma.value == 1.0001403928170594

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        self.hma.update_raw(1.00020)
        self.hma.update_raw(1.00030)
        self.hma.update_raw(1.00050)

        # Act
        self.hma.reset()

        # Assert
        assert not self.hma.initialized
