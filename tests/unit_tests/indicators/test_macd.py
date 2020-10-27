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

from nautilus_trader.indicators.macd import MovingAverageConvergenceDivergence


class MovingAverageConvergenceDivergenceTests(unittest.TestCase):

    # Fixture Setup
    def setUp(self):
        # Arrange
        self.macd = MovingAverageConvergenceDivergence(3, 10)

    def test_name_returns_expected_string(self):
        # Act
        # Assert
        self.assertEqual("MovingAverageConvergenceDivergence", self.macd.name)

    def test_str_repr_returns_expected_string(self):
        # Act
        # Assert
        self.assertEqual("MovingAverageConvergenceDivergence(3, 10, EXPONENTIAL)", str(self.macd))
        self.assertEqual("MovingAverageConvergenceDivergence(3, 10, EXPONENTIAL)", repr(self.macd))

    def test_initialized_without_inputs_returns_false(self):
        # Act
        # Assert
        self.assertEqual(False, self.macd.initialized)

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        self.macd.update_raw(1.00000)
        self.macd.update_raw(2.00000)
        self.macd.update_raw(3.00000)
        self.macd.update_raw(4.00000)
        self.macd.update_raw(5.00000)
        self.macd.update_raw(6.00000)
        self.macd.update_raw(7.00000)
        self.macd.update_raw(8.00000)
        self.macd.update_raw(9.00000)
        self.macd.update_raw(10.00000)
        self.macd.update_raw(11.00000)
        self.macd.update_raw(12.00000)
        self.macd.update_raw(13.00000)
        self.macd.update_raw(14.00000)
        self.macd.update_raw(15.00000)
        self.macd.update_raw(16.00000)

        # Act
        # Assert
        self.assertEqual(True, self.macd.initialized)

    def test_value_with_one_input_returns_expected_value(self):
        # Arrange
        self.macd.update_raw(1.00000)

        # Act
        # Assert
        self.assertEqual(0, self.macd.value)

    def test_value_with_three_inputs_returns_expected_value(self):
        # Arrange
        self.macd.update_raw(1.00000)
        self.macd.update_raw(2.00000)
        self.macd.update_raw(3.00000)

        # Act
        # Assert
        self.assertEqual(0.7376033057851243, self.macd.value)

    def test_value_with_more_inputs_expected_value(self):
        # Arrange
        self.macd.update_raw(1.00000)
        self.macd.update_raw(2.00000)
        self.macd.update_raw(3.00000)
        self.macd.update_raw(4.00000)
        self.macd.update_raw(5.00000)
        self.macd.update_raw(6.00000)
        self.macd.update_raw(7.00000)
        self.macd.update_raw(8.00000)
        self.macd.update_raw(9.00000)
        self.macd.update_raw(10.00000)
        self.macd.update_raw(11.00000)
        self.macd.update_raw(12.00000)
        self.macd.update_raw(13.00000)
        self.macd.update_raw(14.00000)
        self.macd.update_raw(15.00000)
        self.macd.update_raw(16.00000)

        # Act
        # Assert
        self.assertEqual(3.2782313673122907, self.macd.value)

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        self.macd.update_raw(1.00020)
        self.macd.update_raw(1.00030)
        self.macd.update_raw(1.00050)

        # Act
        self.macd.reset()  # No assertion errors.
