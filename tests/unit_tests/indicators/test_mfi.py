# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.indicators.mfi import MoneyFlowIndex
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs


class TestMoneyFlowIndex:
    """Test cases for Money Flow Index (MFI) indicator."""

    def setup_method(self):
        # Fixture setup
        self.mfi = MoneyFlowIndex(14)

    def test_name_returns_expected_string(self):
        # Arrange, Act, Assert
        assert self.mfi.name == "MoneyFlowIndex"

    def test_str_repr_returns_expected_string(self):
        # Arrange, Act, Assert
        assert str(self.mfi) == "MoneyFlowIndex(14)"
        assert repr(self.mfi) == "MoneyFlowIndex(14)"

    def test_period_returns_expected_value(self):
        # Arrange, Act, Assert
        assert self.mfi.period == 14

    def test_initialized_without_inputs_returns_false(self):
        # Arrange, Act, Assert
        assert self.mfi.initialized is False

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        for i in range(14):
            self.mfi.update(
                close=100.0 + i,
                high=101.0 + i,
                low=99.0 + i,
                volume=1000.0 + i * 10,
            )

        # Act, Assert
        assert self.mfi.initialized is True

    def test_handle_bar_updates_indicator(self):
        # Arrange
        indicator = MoneyFlowIndex(10)
        bar = TestDataStubs.bar_5decimal()

        # Act
        indicator.handle_bar(bar)

        # Assert
        assert indicator.has_inputs
        assert indicator.value == 0.5  # First value should be neutral

    def test_value_with_one_input(self):
        # Arrange, Act
        self.mfi.update(close=100.0, high=101.0, low=99.0, volume=1000.0)

        # Assert
        assert self.mfi.value == 0.5  # First value is neutral

    def test_value_with_increasing_prices(self):
        # Arrange
        # Simulate increasing prices (bullish pressure)
        for i in range(20):
            self.mfi.update(
                close=100.0 + i,
                high=101.0 + i,
                low=99.0 + i,
                volume=1000.0,
            )

        # Act, Assert
        assert self.mfi.value > 0.5  # Should be above neutral

    def test_value_with_decreasing_prices(self):
        # Arrange
        # First seed with a high value
        self.mfi.update(close=120.0, high=121.0, low=119.0, volume=1000.0)
        
        # Then simulate decreasing prices (bearish pressure)
        for i in range(20):
            self.mfi.update(
                close=119.0 - i,
                high=120.0 - i,
                low=118.0 - i,
                volume=1000.0,
            )

        # Act, Assert
        assert self.mfi.value < 0.5  # Should be below neutral

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        for i in range(20):
            self.mfi.update(
                close=100.0 + i,
                high=101.0 + i,
                low=99.0 + i,
                volume=1000.0,
            )

        # Act
        self.mfi.reset()

        # Assert
        assert not self.mfi.initialized
        assert self.mfi.value == 0.0  # Reset value

    def test_with_nan_input(self):
        # Arrange
        self.mfi.update(close=100.0, high=101.0, low=99.0, volume=1000.0)
        
        # Act
        self.mfi.update(close=float('nan'), high=101.0, low=99.0, volume=1000.0)
        
        # Assert - should handle gracefully
        assert self.mfi.has_inputs

    def test_with_zero_volume(self):
        # Arrange
        self.mfi.update(close=100.0, high=101.0, low=99.0, volume=1000.0)
        
        # Act
        self.mfi.update(close=101.0, high=102.0, low=100.0, volume=0.0)
        
        # Assert - should handle zero volume
        assert self.mfi.has_inputs
