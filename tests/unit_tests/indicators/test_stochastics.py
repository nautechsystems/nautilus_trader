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

from nautilus_trader.indicators.stochastics import Stochastics
from nautilus_trader.test_kit.stubs.data import TestDataStubs


class TestStochastics:
    def setup(self):
        # Fixture Setup
        self.stochastics = Stochastics(14, 3)

    def test_name_returns_expected_string(self):
        # Act, Assert
        assert self.stochastics.name == "Stochastics"

    def test_str_repr_returns_expected_string(self):
        # Act, Assert
        assert str(self.stochastics) == "Stochastics(14, 3)"
        assert repr(self.stochastics) == "Stochastics(14, 3)"

    def test_period_k_returns_expected_value(self):
        # Act, Assert
        assert self.stochastics.period_k == 14

    def test_period_d_returns_expected_value(self):
        # Act, Assert
        assert self.stochastics.period_d == 3

    def test_initialized_without_inputs_returns_false(self):
        # Act, Assert
        assert self.stochastics.initialized is False

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        self.stochastics.update_raw(1.00020, 1.00000, 1.00010)
        self.stochastics.update_raw(1.00020, 1.00000, 1.00010)
        self.stochastics.update_raw(1.00020, 1.00000, 1.00010)
        self.stochastics.update_raw(1.00020, 1.00000, 1.00010)
        self.stochastics.update_raw(1.00020, 1.00000, 1.00010)
        self.stochastics.update_raw(1.00020, 1.00000, 1.00010)
        self.stochastics.update_raw(1.00020, 1.00000, 1.00010)
        self.stochastics.update_raw(1.00020, 1.00000, 1.00010)
        self.stochastics.update_raw(1.00020, 1.00000, 1.00010)
        self.stochastics.update_raw(1.00020, 1.00000, 1.00010)
        self.stochastics.update_raw(1.00020, 1.00000, 1.00010)
        self.stochastics.update_raw(1.00020, 1.00000, 1.00010)
        self.stochastics.update_raw(1.00020, 1.00000, 1.00010)
        self.stochastics.update_raw(1.00020, 1.00000, 1.00010)

        # Act, Assert
        assert self.stochastics.initialized is True

    def test_handle_bar_updates_indicator(self):
        # Arrange
        indicator = Stochastics(14, 3)

        bar = TestDataStubs.bar_5decimal()

        # Act
        indicator.handle_bar(bar)

        # Assert
        assert indicator.has_inputs
        assert indicator.value_k == 66.66666666641994
        assert indicator.value_d == 66.66666666641994

    def test_values_with_one_input_returns_expected_value(self):
        # Arrange
        self.stochastics.update_raw(1.00020, 1.00000, 1.00010)

        # Act, Assert
        assert self.stochastics.value_k == 50.0
        assert self.stochastics.value_d == 50.0

    def test_value_with_all_higher_inputs_returns_expected_value(self):
        # Arrange
        self.stochastics.update_raw(1.00020, 1.00000, 1.00010)
        self.stochastics.update_raw(1.00030, 1.00010, 1.00020)
        self.stochastics.update_raw(1.00040, 1.00020, 1.00030)
        self.stochastics.update_raw(1.00050, 1.00030, 1.00040)

        # Act, Assert
        assert self.stochastics.value_k == 80.0
        assert self.stochastics.value_d == 75.0

    def test_value_with_all_lower_inputs_returns_expected_value(self):
        # Arrange
        self.stochastics.update_raw(1.00050, 1.00030, 1.00040)
        self.stochastics.update_raw(1.00040, 1.00020, 1.00030)
        self.stochastics.update_raw(1.00030, 1.00010, 1.00020)
        self.stochastics.update_raw(1.00020, 1.00000, 1.00010)

        # Act, Assert
        assert self.stochastics.value_k == 20.0
        assert self.stochastics.value_d == 25.0

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        self.stochastics.update_raw(1.00050, 1.00030, 1.00040)

        # Act
        self.stochastics.reset()  # No assertion errors

        # Assert
        assert not self.stochastics.initialized
        assert self.stochastics.value_k == 0
        assert self.stochastics.value_d == 0
