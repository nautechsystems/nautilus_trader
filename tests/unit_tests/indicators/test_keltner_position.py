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

from nautilus_trader.indicators.keltner_position import KeltnerPosition
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestKeltnerPosition:
    def setup(self):
        # Fixture Setup
        self.kp = KeltnerPosition(10, 2.5)

    def test_name_returns_expected_string(self):
        # Arrange, Act, Assert
        assert self.kp.name == "KeltnerPosition"

    def test_str_repr_returns_expected_string(self):
        # Arrange, Act, Assert
        assert str(self.kp) == "KeltnerPosition(10, 2.5, EXPONENTIAL, SIMPLE, True, 0.0)"
        assert repr(self.kp) == "KeltnerPosition(10, 2.5, EXPONENTIAL, SIMPLE, True, 0.0)"

    def test_initialized_without_inputs_returns_false(self):
        # Arrange, Act, Assert
        assert self.kp.initialized is False

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        for _i in range(10):
            self.kp.update_raw(1.00000, 1.00000, 1.00000)

        # Act, Assert
        assert self.kp.initialized is True

    def test_period_returns_expected_value(self):
        # Arrange, Act, Assert
        assert self.kp.period == 10

    def test_k_multiple_returns_expected_value(self):
        # Arrange, Act, Assert
        assert self.kp.k_multiplier == 2.5

    def test_handle_bar_updates_indicator(self):
        # Arrange
        indicator = KeltnerPosition(10, 2.5)

        bar = TestDataStubs.bar_5decimal()

        # Act
        indicator.handle_bar(bar)

        # Assert
        assert indicator.has_inputs
        assert indicator.value == 0.0444444444447405

    def test_value_with_one_input_returns_zero(self):
        # Arrange
        self.kp.update_raw(1.00020, 1.00000, 1.00010)

        # Act, Assert
        assert self.kp.value == 0

    def test_value_with_zero_width_input_returns_zero(self):
        # Arrange
        for _i in range(10):
            self.kp.update_raw(1.00000, 1.00000, 1.00000)

        # Act, Assert
        assert self.kp.value == 0

    def test_value_with_three_inputs_returns_expected_value(self):
        # Arrange
        self.kp.update_raw(1.00020, 1.00000, 1.00010)
        self.kp.update_raw(1.00030, 1.00010, 1.00020)
        self.kp.update_raw(1.00040, 1.00020, 1.00030)

        # Act, Assert
        assert self.kp.value == 0.29752066115754594

    def test_value_with_close_on_high_returns_positive_value(self):
        # Arrange
        high = 1.00010
        low = 1.00000

        for _i in range(10):
            high += 0.00010
            low += 0.00010
            close = high
            self.kp.update_raw(high, low, close)

        # Act, Assert
        assert self.kp.value == 1.637585941284833

    def test_value_with_close_on_low_returns_lower_value(self):
        # Arrange
        high = 1.00010
        low = 1.00000

        for _i in range(10):
            high -= 0.00010
            low -= 0.00010
            close = low
            self.kp.update_raw(high, low, close)

        # Act, Assert
        assert self.kp.value == pytest.approx(-1.637585941284833, rel=1e-9)

    def test_value_with_ten_inputs_returns_expected_value(self):
        # Arrange
        self.kp.update_raw(1.00020, 1.00000, 1.00010)
        self.kp.update_raw(1.00030, 1.00010, 1.00020)
        self.kp.update_raw(1.00050, 1.00020, 1.00030)
        self.kp.update_raw(1.00030, 1.00000, 1.00010)
        self.kp.update_raw(1.00030, 1.00010, 1.00020)
        self.kp.update_raw(1.00040, 1.00020, 1.00030)
        self.kp.update_raw(1.00010, 1.00000, 1.00010)
        self.kp.update_raw(1.00030, 1.00010, 1.00020)
        self.kp.update_raw(1.00030, 1.00020, 1.00030)
        self.kp.update_raw(1.00020, 1.00010, 1.00010)

        # Act, Assert
        assert self.kp.value == pytest.approx(-0.14281747514671334, rel=1e-9)

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        self.kp.update_raw(1.00020, 1.00000, 1.00010)
        self.kp.update_raw(1.00030, 1.00010, 1.00020)
        self.kp.update_raw(1.00040, 1.00020, 1.00030)

        # Act
        self.kp.reset()

        # Assert
        assert not self.kp.initialized
