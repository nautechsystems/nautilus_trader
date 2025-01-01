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

from nautilus_trader.indicators.average.moving_average import MovingAverageType
from nautilus_trader.indicators.average.vidya import VariableIndexDynamicAverage
from nautilus_trader.model.enums import PriceType
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestVariableIndexDynamicAverage:
    def setup(self):
        # Fixture Setup
        self.vida = VariableIndexDynamicAverage(period=10, cmo_ma_type=MovingAverageType.SIMPLE)

    def test_name_returns_expected_string(self):
        # Arrange, Act, Assert
        assert self.vida.name == "VariableIndexDynamicAverage"

    def test_str_repr_returns_expected_string(self):
        # Arrange, Act, Assert
        assert str(self.vida) == "VariableIndexDynamicAverage(10)"
        assert repr(self.vida) == "VariableIndexDynamicAverage(10)"

    def test_period_returns_expected_value(self):
        # Arrange, Act, Assert
        assert self.vida.period == 10

    def test_initialized_without_inputs_returns_false(self):
        # Arrange, Act, Assert
        assert self.vida.initialized is False

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange, Act
        self.vida.update_raw(13.102)
        self.vida.update_raw(12.993)
        self.vida.update_raw(13.008)
        self.vida.update_raw(12.716)
        self.vida.update_raw(12.807)
        self.vida.update_raw(12.912)
        self.vida.update_raw(12.965)
        self.vida.update_raw(12.900)
        self.vida.update_raw(12.991)
        self.vida.update_raw(13.066)
        self.vida.update_raw(13.205)
        self.vida.update_raw(13.234)
        self.vida.update_raw(13.249)
        self.vida.update_raw(13.295)
        self.vida.update_raw(13.400)
        self.vida.update_raw(13.587)
        self.vida.update_raw(13.320)
        self.vida.update_raw(13.250)
        self.vida.update_raw(13.444)
        self.vida.update_raw(13.335)

        # Assert
        assert self.vida.value == pytest.approx(7.656223577745644, rel=1e-9)
        assert self.vida.initialized is True

    def test_handle_quote_tick_updates_indicator(self):
        # Arrange
        indicator = VariableIndexDynamicAverage(10, PriceType.MID)
        tick = TestDataStubs.quote_tick()

        # Act
        indicator.handle_quote_tick(tick)

        # Assert
        assert indicator.has_inputs
        assert indicator.value == 0

    def test_handle_trade_tick_updates_indicator(self):
        # Arrange
        indicator = VariableIndexDynamicAverage(10)
        tick = TestDataStubs.trade_tick()

        # Act
        indicator.handle_trade_tick(tick)

        # Assert
        assert indicator.has_inputs
        assert indicator.value == 0

    def test_handle_bar_updates_indicator(self):
        # Arrange
        indicator = VariableIndexDynamicAverage(10)
        bar = TestDataStubs.bar_5decimal()

        # Act
        indicator.handle_bar(bar)

        # Assert
        assert indicator.has_inputs
        assert indicator.value == 0

    def test_value_with_one_input_returns_expected_value(self):
        # Arrange, Act
        self.vida.update_raw(1.0)

        # Assert
        assert self.vida.value == 0

    def test_value_with_three_inputs_returns_expected_value(self):
        # Arrange, Act
        self.vida.update_raw(1.0)
        self.vida.update_raw(2.0)
        self.vida.update_raw(3.0)

        # Assert
        assert self.vida.value == 0

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        for _i in range(1000):
            self.vida.update_raw(1.0)

        # Act
        self.vida.reset()

        # Assert
        assert not self.vida.initialized
        assert self.vida.value == 0.0
