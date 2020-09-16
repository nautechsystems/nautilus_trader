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

import sys
import unittest

from nautilus_trader.indicators.atr import AverageTrueRange
from tests.test_kit.series import BatterySeries


class AverageTrueRangeTests(unittest.TestCase):

    # Fixture Setup
    def setUp(self):
        # Arrange
        self.atr = AverageTrueRange(10)

    def test_name(self):
        # Act
        # Assert
        self.assertEqual('AverageTrueRange', self.atr.name)

    def test_str(self):
        # Act
        # Assert
        self.assertEqual('AverageTrueRange(10, SIMPLE, True, 0.0)', str(self.atr))

    def test_repr(self):
        # Act
        # Assert
        self.assertTrue(repr(self.atr).startswith('<AverageTrueRange(10, SIMPLE, True, 0.0) object at'))
        self.assertTrue(repr(self.atr).endswith('>'))

    def test_period(self):
        # Act
        # Assert
        self.assertEqual(10, self.atr.period)

    def test_initialized_without_inputs_returns_false(self):
        # Act
        # Assert
        self.assertEqual(False, self.atr.initialized)

    def test_initialized_with_required_inputs_returns_true(self):
        # Act
        for _i in range(10):
            self.atr.update_raw(1.00000, 1.00000, 1.00000)

        # Assert
        self.assertEqual(True, self.atr.initialized)

    def test_value_with_no_inputs_returns_zero(self):
        # Act
        # Assert
        self.assertEqual(0.0, self.atr.value)

    def test_value_with_epsilon_input(self):
        # Arrange
        epsilon = sys.float_info.epsilon
        self.atr.update_raw(epsilon, epsilon, epsilon)

        # Act
        # Assert
        self.assertEqual(0.0, self.atr.value)

    def test_value_with_one_ones_input(self):
        # Arrange
        self.atr.update_raw(1.00000, 1.00000, 1.00000)

        # Act
        # Assert
        self.assertEqual(0.0, self.atr.value)

    def test_value_with_one_input(self):
        # Arrange
        self.atr.update_raw(1.00020, 1.00000, 1.00010)

        # Act
        # Assert
        self.assertAlmostEqual(0.00020, self.atr.value)

    def test_value_with_three_inputs(self):
        # Arrange
        self.atr.update_raw(1.00020, 1.00000, 1.00010)
        self.atr.update_raw(1.00020, 1.00000, 1.00010)
        self.atr.update_raw(1.00020, 1.00000, 1.00010)

        # Act
        # Assert
        self.assertAlmostEqual(0.00020, self.atr.value)

    def test_value_with_close_on_high(self):
        # Arrange
        high = 1.00010
        low = 1.00000

        # Act
        for _i in range(1000):
            high += 0.00010
            low += 0.00010
            close = high
            self.atr.update_raw(high, low, close)

        # Assert
        self.assertAlmostEqual(0.00010, self.atr.value, 2)

    def test_value_with_close_on_low(self):
        # Arrange
        high = 1.00010
        low = 1.00000

        # Act
        for _i in range(1000):
            high -= 0.00010
            low -= 0.00010
            close = low
            self.atr.update_raw(high, low, close)

        # Assert
        self.assertAlmostEqual(0.00010, self.atr.value)

    def test_floor_with_ten_ones_inputs(self):
        # Arrange
        floor = 0.00005
        floored_atr = AverageTrueRange(10, value_floor=floor)

        for _i in range(20):
            floored_atr.update_raw(1.00000, 1.00000, 1.00000)

        # Act
        # Assert
        self.assertEqual(5e-05, floored_atr.value)

    def test_floor_with_exponentially_decreasing_high_inputs(self):
        # Arrange
        floor = 0.00005
        floored_atr = AverageTrueRange(10, value_floor=floor)

        high = 1.00020
        low = 1.00000
        close = 1.00000

        for _i in range(20):
            high -= (high - low) / 2
            floored_atr.update_raw(high, low, close)

        # Act
        # Assert
        self.assertEqual(5e-05, floored_atr.value)

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        for _i in range(1000):
            self.atr.update_raw(1.00010, 1.00000, 1.00005)

        # Act
        self.atr.reset()

        # Assert
        self.assertEqual(0.0, self.atr.value)  # No assertion errors.

    def test_with_battery_signal(self):
        # Arrange
        battery_signal = BatterySeries.create()
        output = []

        # Act
        for point in BatterySeries.create():
            self.atr.update_raw(point, sys.float_info.epsilon, sys.float_info.epsilon)
            output.append(self.atr.value)

        # Assert
        self.assertEqual(len(battery_signal), len(output))
