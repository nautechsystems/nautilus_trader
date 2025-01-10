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
from nautilus_trader.indicators.pressure import Pressure
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestPressure:
    def setup(self):
        # Fixture Setup
        self.pressure = Pressure(10, MovingAverageType.EXPONENTIAL)

    def test_name_returns_expected_string(self):
        # Arrange, Act, Assert
        assert self.pressure.name == "Pressure"

    def test_str_repr_returns_expected_string(self):
        # Arrange, Act, Assert
        assert str(self.pressure) == "Pressure(10, EXPONENTIAL, 0.0)"
        assert repr(self.pressure) == "Pressure(10, EXPONENTIAL, 0.0)"

    def test_period_returns_expected_value(self):
        # Arrange, Act, Assert
        assert self.pressure.period == 10

    def test_initialized_without_inputs_returns_false(self):
        # Arrange, Act, Assert
        assert self.pressure.initialized is False

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        for _i in range(10):
            self.pressure.update_raw(1.00000, 1.00000, 1.00000, 1000)

        # Act, Assert
        assert self.pressure.initialized is True

    def test_handle_bar_updates_indicator(self):
        # Arrange
        indicator = Pressure(10, MovingAverageType.EXPONENTIAL)

        bar = TestDataStubs.bar_5decimal()

        # Act
        indicator.handle_bar(bar)

        # Assert
        assert indicator.has_inputs
        assert indicator.value == pytest.approx(0.333333333328399, rel=1e-9)

    def test_value_with_one_input_returns_expected_value(self):
        # Arrange
        self.pressure.update_raw(1.00000, 1.00000, 1.00000, 1000)

        # Act, Assert
        assert self.pressure.value == 0

    def test_values_with_higher_inputs_returns_expected_value(self):
        # Arrange
        self.pressure.update_raw(1.00010, 1.00000, 1.00010, 1000)
        self.pressure.update_raw(1.00020, 1.00000, 1.00020, 1000)
        self.pressure.update_raw(1.00030, 1.00000, 1.00030, 1000)
        self.pressure.update_raw(1.00040, 1.00000, 1.00040, 1000)
        self.pressure.update_raw(1.00050, 1.00000, 1.00050, 1000)
        self.pressure.update_raw(1.00060, 1.00000, 1.00060, 1000)
        self.pressure.update_raw(1.00070, 1.00000, 1.00070, 1000)
        self.pressure.update_raw(1.00080, 1.00000, 1.00080, 1000)
        self.pressure.update_raw(1.00090, 1.00000, 1.00090, 1000)
        self.pressure.update_raw(1.00100, 1.00000, 1.00100, 1000)

        # Act, Assert
        assert self.pressure.value == 1.6027263066543116
        assert self.pressure.value_cumulative == 17.427420446202998

    def test_values_with_all_lower_inputs_returns_expected_value(self):
        # Arrange
        self.pressure.update_raw(1.00000, 0.99990, 0.99990, 1000)
        self.pressure.update_raw(1.00000, 0.99980, 0.99980, 1000)
        self.pressure.update_raw(1.00000, 0.99970, 0.99970, 1000)
        self.pressure.update_raw(1.00000, 0.99960, 0.99960, 1000)
        self.pressure.update_raw(1.00000, 0.99950, 0.99950, 1000)
        self.pressure.update_raw(1.00000, 0.99940, 0.99940, 1000)
        self.pressure.update_raw(1.00000, 0.99930, 0.99930, 1000)
        self.pressure.update_raw(1.00000, 0.99920, 0.99920, 1000)
        self.pressure.update_raw(1.00000, 0.99910, 0.99910, 1000)
        self.pressure.update_raw(1.00000, 0.99900, 0.99900, 1000)

        # Act, Assert
        assert self.pressure.value == -1.602726306654309
        assert self.pressure.value_cumulative == -17.427420446203406

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        for _i in range(10):
            self.pressure.update_raw(1.00000, 1.00000, 1.00000, 1000)

        # Act
        self.pressure.reset()

        # Assert
        assert not self.pressure.initialized
