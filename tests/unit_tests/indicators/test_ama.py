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

from nautilus_trader.indicators.average.ama import AdaptiveMovingAverage
from nautilus_trader.model.enums import PriceType
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestAdaptiveMovingAverage:
    def setup(self):
        # Fixture Setup
        self.ama = AdaptiveMovingAverage(10, 2, 30)

    def test_name_returns_expected_string(self):
        # Arrange, Act, Assert
        assert self.ama.name == "AdaptiveMovingAverage"

    def test_str_repr_returns_expected_string(self):
        # Arrange, Act, Assert
        assert str(self.ama) == "AdaptiveMovingAverage(10, 2, 30)"
        assert repr(self.ama) == "AdaptiveMovingAverage(10, 2, 30)"

    def test_period(self):
        # Arrange, Act, Assert
        assert self.ama.period == 10

    def test_initialized_without_inputs_returns_false(self):
        # Arrange, Act, Assert
        assert self.ama.initialized is False

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        # Arrange, Act
        for _i in range(10):
            self.ama.update_raw(1.0)

        # Assert
        assert self.ama.initialized is True

    def test_handle_quote_tick_updates_indicator(self):
        # Arrange
        indicator = AdaptiveMovingAverage(10, 2, 30, PriceType.MID)

        tick = TestDataStubs.quote_tick()

        # Act
        indicator.handle_quote_tick(tick)

        # Assert
        assert indicator.has_inputs
        assert indicator.value == 1.0

    def test_handle_trade_tick_updates_indicator(self):
        # Arrange
        indicator = AdaptiveMovingAverage(10, 2, 30)

        tick = TestDataStubs.trade_tick()

        # Act
        indicator.handle_trade_tick(tick)

        # Assert
        assert indicator.has_inputs
        assert indicator.value == 1.0

    def test_handle_bar_updates_indicator(self):
        # Arrange
        indicator = AdaptiveMovingAverage(10, 2, 30)

        bar = TestDataStubs.bar_5decimal()

        # Act
        indicator.handle_bar(bar)

        # Assert
        assert indicator.has_inputs
        assert indicator.value == 1.00003

    def test_value_with_one_input(self):
        # Arrange
        self.ama.update_raw(1.0)

        # Act, Assert
        assert self.ama.value == 1.0

    def test_value_with_three_inputs(self):
        # Arrange
        self.ama.update_raw(1.0)
        self.ama.update_raw(2.0)
        self.ama.update_raw(3.0)

        # Act, Assert
        assert self.ama.value == 2.135802469135802

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        for _i in range(1000):
            self.ama.update_raw(1.0)

        # Act
        self.ama.reset()

        # Assert
        assert not self.ama.initialized
        assert self.ama.value == 0
