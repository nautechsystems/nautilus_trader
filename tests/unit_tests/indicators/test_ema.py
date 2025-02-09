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


import pytest

from nautilus_trader.indicators.average.ema import ExponentialMovingAverage
from nautilus_trader.model.enums import PriceType
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestExponentialMovingAverage:
    def setup(self):
        # Fixture Setup
        self.ema = ExponentialMovingAverage(10)

    def test_name_returns_expected_string(self):
        # Arrange, Act, Assert
        assert self.ema.name == "ExponentialMovingAverage"

    def test_str_repr_returns_expected_string(self):
        # Arrange, Act, Assert
        assert str(self.ema) == "ExponentialMovingAverage(10)"
        assert repr(self.ema) == "ExponentialMovingAverage(10)"

    def test_period_returns_expected_value(self):
        # Arrange, Act, Assert
        assert self.ema.period == 10

    def test_multiplier_returns_expected_value(self):
        # Arrange, Act, Assert
        assert self.ema.alpha == 0.18181818181818182

    def test_initialized_without_inputs_returns_false(self):
        # Arrange, Act, Assert
        assert self.ema.initialized is False

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        self.ema.update_raw(1.00000)
        self.ema.update_raw(2.00000)
        self.ema.update_raw(3.00000)
        self.ema.update_raw(4.00000)
        self.ema.update_raw(5.00000)
        self.ema.update_raw(6.00000)
        self.ema.update_raw(7.00000)
        self.ema.update_raw(8.00000)
        self.ema.update_raw(9.00000)
        self.ema.update_raw(10.00000)

        # Act

        # Assert
        assert self.ema.initialized is True

    def test_handle_quote_tick_updates_indicator(self):
        # Arrange
        indicator = ExponentialMovingAverage(10, PriceType.MID)

        tick = TestDataStubs.quote_tick()

        # Act
        indicator.handle_quote_tick(tick)

        # Assert
        assert indicator.has_inputs
        assert indicator.value == 1.0

    def test_handle_trade_tick_updates_indicator(self):
        # Arrange
        indicator = ExponentialMovingAverage(10)

        tick = TestDataStubs.trade_tick()

        # Act
        indicator.handle_trade_tick(tick)

        # Assert
        assert indicator.has_inputs
        assert indicator.value == 1.0

    def test_handle_bar_updates_indicator(self):
        # Arrange
        indicator = ExponentialMovingAverage(10)

        bar = TestDataStubs.bar_5decimal()

        # Act
        indicator.handle_bar(bar)

        # Assert
        assert indicator.has_inputs
        assert indicator.value == 1.00003

    def test_value_with_one_input_returns_expected_value(self):
        # Arrange
        self.ema.update_raw(1.00000)

        # Act, Assert
        assert self.ema.value == 1.0

    def test_value_with_three_inputs_returns_expected_value(self):
        # Arrange
        self.ema.update_raw(1.00000)
        self.ema.update_raw(2.00000)
        self.ema.update_raw(3.00000)

        # Act, Assert
        assert self.ema.value == pytest.approx(1.5123966942148757, rel=1e-9)

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        for _i in range(1000):
            self.ema.update_raw(1.00000)

        # Act
        self.ema.reset()

        # Assert
        assert not self.ema.initialized
        assert self.ema.value == 0.0
