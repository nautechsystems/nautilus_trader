# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

import unittest

from nautilus_trader.indicators.stochastics import Stochastics


class StochasticsTests(unittest.TestCase):

    # Fixture Setup
    def setUp(self):
        # Arrange
        self.stochastics = Stochastics(14, 3)

    def test_name_returns_expected_string(self):
        # Act
        # Assert
        self.assertEqual("Stochastics", self.stochastics.name)

    def test_str_repr_returns_expected_string(self):
        # Act
        # Assert
        self.assertEqual("Stochastics(14, 3)", str(self.stochastics))
        self.assertEqual("Stochastics(14, 3)", repr(self.stochastics))

    def test_period_k_returns_expected_value(self):
        # Act
        # Assert
        self.assertEqual(14, self.stochastics.period_k)

    def test_period_d_returns_expected_value(self):
        # Act
        # Assert
        self.assertEqual(3, self.stochastics.period_d)

    def test_initialized_without_inputs_returns_false(self):
        # Act
        # Assert
        self.assertEqual(False, self.stochastics.initialized)

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

        # Act
        # Assert
        self.assertEqual(True, self.stochastics.initialized)

    def test_values_with_one_input_returns_expected_value(self):
        # Arrange
        self.stochastics.update_raw(1.00020, 1.00000, 1.00010)

        # Act
        # Assert
        self.assertEqual(50.0, self.stochastics.value_k)
        self.assertEqual(50.0, self.stochastics.value_d)

    def test_value_with_all_higher_inputs_returns_expected_value(self):
        # Arrange
        self.stochastics.update_raw(1.00020, 1.00000, 1.00010)
        self.stochastics.update_raw(1.00030, 1.00010, 1.00020)
        self.stochastics.update_raw(1.00040, 1.00020, 1.00030)
        self.stochastics.update_raw(1.00050, 1.00030, 1.00040)

        # Act
        # Assert
        self.assertEqual(80.0, self.stochastics.value_k)
        self.assertEqual(75.0, self.stochastics.value_d)

    def test_value_with_all_lower_inputs_returns_expected_value(self):
        # Arrange
        self.stochastics.update_raw(1.00050, 1.00030, 1.00040)
        self.stochastics.update_raw(1.00040, 1.00020, 1.00030)
        self.stochastics.update_raw(1.00030, 1.00010, 1.00020)
        self.stochastics.update_raw(1.00020, 1.00000, 1.00010)

        # Act
        # Assert
        self.assertEqual(20.0, self.stochastics.value_k)
        self.assertEqual(25.0, self.stochastics.value_d)

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        self.stochastics.update_raw(1.00050, 1.00030, 1.00040)

        # Act
        self.stochastics.reset()  # No assertion errors

        # Assert
        self.assertFalse(self.stochastics.initialized)
        self.assertEqual(0, self.stochastics.value_k)
        self.assertEqual(0, self.stochastics.value_d)
