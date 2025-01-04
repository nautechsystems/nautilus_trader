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

from nautilus_trader.indicators.aroon import AroonOscillator
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestAroonOscillator:
    def setup(self):
        # Fixture Setup
        self.aroon = AroonOscillator(10)

    def test_name_returns_expected_string(self):
        # Arrange, Act, Assert
        assert self.aroon.name == "AroonOscillator"

    def test_period(self):
        # Arrange, Act, Assert
        assert self.aroon.period == 10

    def test_initialized_without_inputs_returns_false(self):
        # Arrange, Act, Assert
        assert self.aroon.initialized is False

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange, Act
        for _i in range(20):
            self.aroon.update_raw(110.08, 109.61)

        # Assert
        assert self.aroon.initialized is True

    def test_handle_bar_updates_indicator(self):
        # Arrange
        indicator = AroonOscillator(10)
        bar = TestDataStubs.bar_5decimal()

        # Act
        indicator.handle_bar(bar)

        # Assert
        assert indicator.has_inputs
        assert indicator.aroon_up == 100.0
        assert indicator.aroon_down == 100.0
        assert indicator.value == 0

    def test_value_with_one_input(self):
        # Arrange, Act
        self.aroon.update_raw(110.08, 109.61)

        # Assert
        assert self.aroon.aroon_up == 100.0
        assert self.aroon.aroon_down == 100.0
        assert self.aroon.value == 0

    def test_value_with_twenty_inputs(self):
        # Arrange, Act
        self.aroon.update_raw(110.08, 109.61)
        self.aroon.update_raw(110.15, 109.91)
        self.aroon.update_raw(110.1, 109.73)
        self.aroon.update_raw(110.06, 109.77)
        self.aroon.update_raw(110.29, 109.88)
        self.aroon.update_raw(110.53, 110.29)
        self.aroon.update_raw(110.61, 110.26)
        self.aroon.update_raw(110.28, 110.17)
        self.aroon.update_raw(110.3, 110.0)
        self.aroon.update_raw(110.25, 110.01)
        self.aroon.update_raw(110.25, 109.81)
        self.aroon.update_raw(109.92, 109.71)
        self.aroon.update_raw(110.21, 109.84)
        self.aroon.update_raw(110.08, 109.95)
        self.aroon.update_raw(110.2, 109.96)
        self.aroon.update_raw(110.16, 109.95)
        self.aroon.update_raw(109.99, 109.75)
        self.aroon.update_raw(110.2, 109.73)
        self.aroon.update_raw(110.1, 109.81)
        self.aroon.update_raw(110.04, 109.96)

        # Assert
        assert self.aroon.aroon_up == 9.999999999999998
        assert self.aroon.aroon_down == 19.999999999999996
        assert self.aroon.value == -9.999999999999998

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        for _i in range(1000):
            self.aroon.update_raw(110.08, 109.61)

        # Act
        self.aroon.reset()

        # Assert
        assert not self.aroon.initialized
        assert self.aroon.aroon_up == 0
        assert self.aroon.aroon_down == 0
        assert self.aroon.value == 0
