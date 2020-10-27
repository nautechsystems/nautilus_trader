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

from nautilus_trader.indicators.average.ama import AdaptiveMovingAverage


class AdaptiveMovingAverageTests(unittest.TestCase):

    # Fixture Setup
    def setUp(self):
        # Arrange
        self.ama = AdaptiveMovingAverage(10, 2, 30)

    def test_name_returns_expected_string(self):
        # Act
        # Assert
        self.assertEqual("AdaptiveMovingAverage", self.ama.name)

    def test_str_repr_returns_expected_string(self):
        # Act
        # Assert
        self.assertEqual("AdaptiveMovingAverage(10, 2, 30)", str(self.ama))
        self.assertEqual("AdaptiveMovingAverage(10, 2, 30)", repr(self.ama))

    def test_period(self):
        # Act
        # Assert
        self.assertEqual(10, self.ama.period)

    def test_initialized_without_inputs_returns_false(self):
        # Act
        # Assert
        self.assertEqual(False, self.ama.initialized)

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        # Act
        for _i in range(10):
            self.ama.update_raw(1.00000)

        # Assert
        self.assertEqual(True, self.ama.initialized)

    def test_value_with_one_input(self):
        # Arrange
        self.ama.update_raw(1.00000)

        # Act
        # Assert
        self.assertEqual(1.0, self.ama.value)

    def test_value_with_three_inputs(self):
        # Arrange
        self.ama.update_raw(1.00000)
        self.ama.update_raw(2.00000)
        self.ama.update_raw(3.00000)

        # Act
        # Assert
        self.assertEqual(2.135802469135802, self.ama.value, 10)

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        for _i in range(1000):
            self.ama.update_raw(1.00000)

        # Act
        self.ama.reset()

        # Assert
        self.assertEqual(0.0, self.ama.value)  # No assertion errors.
