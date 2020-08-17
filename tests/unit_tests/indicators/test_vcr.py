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

from nautilus_trader.indicators.vcr import VolatilityCompressionRatio
from tests.test_kit.series import BatterySeries


class VolatilityCompressionRatioTests(unittest.TestCase):

    # Fixture Setup
    def setUp(self):
        # Arrange
        self.vcr = VolatilityCompressionRatio(10, 100)

    def test_name_returns_expected_name(self):
        # Act
        # Assert
        self.assertEqual("VolatilityCompressionRatio", self.vcr.name)

    def test_str_returns_expected_string(self):
        # Act
        # Assert
        self.assertEqual("VolatilityCompressionRatio(10, 100, SIMPLE, True, 0.0)", str(self.vcr))

    def test_repr_returns_expected_string(self):
        # Act
        # Assert
        self.assertTrue(repr(self.vcr).startswith("<VolatilityCompressionRatio(10, 100, SIMPLE, True, 0.0) object at"))
        self.assertTrue(repr(self.vcr).endswith(">"))

    def test_initialized_without_inputs_returns_false(self):
        # Act
        # Assert
        self.assertEqual(False, self.vcr.initialized)

    def test_initialized_with_required_inputs_returns_true(self):
        # Act
        for i in range(100):
            self.vcr.update(1.00000, 1.00000, 1.00000)

        # Assert
        self.assertEqual(True, self.vcr.initialized)

    def test_initialized_with_required_mid_inputs_returns_true(self):
        # Act
        for i in range(100):
            self.vcr.update_mid(1.00000)

        # Assert
        self.assertEqual(True, self.vcr.initialized)

    def test_value_with_no_inputs_returns_none(self):
        # Act
        # Assert
        self.assertEqual(0.0, self.vcr.value)

    def test_value_with_epsilon_inputs_returns_expected_value(self):
        # Arrange
        epsilon = sys.float_info.epsilon
        self.vcr.update(epsilon, epsilon, epsilon)

        # Act
        # Assert
        self.assertEqual(0.0, self.vcr.value)

    def test_value_with_one_ones_input_returns_expected_value(self):
        # Arrange
        self.vcr.update(1.00000, 1.00000, 1.00000)

        # Act
        # Assert
        self.assertEqual(0.0, self.vcr.value)

    def test_value_with_one_input_returns_expected_value(self):
        # Arrange
        self.vcr.update(1.00020, 1.00000, 1.00010)

        # Act
        # Assert
        self.assertEqual(1.0, self.vcr.value)

    def test_value_with_three_inputs_returns_expected_value(self):
        # Arrange
        self.vcr.update(1.00020, 1.00000, 1.00010)
        self.vcr.update(1.00020, 1.00000, 1.00010)
        self.vcr.update(1.00020, 1.00000, 1.00010)

        # Act
        # Assert
        self.assertEqual(1.0, self.vcr.value)

    def test_value_with_three_mid_inputs_returns_expected_value(self):
        # Arrange
        self.vcr.update_mid(1.00000)
        self.vcr.update_mid(1.00020)
        self.vcr.update_mid(1.00040)

        # Act
        # Assert
        self.assertEqual(1.0, self.vcr.value)

    def test_value_with_close_on_high_returns_expected_value(self):
        # Arrange
        high = 1.00010
        low = 1.00000
        factor = 0.0

        # Act
        for i in range(1000):
            high += 0.00010 + factor
            low += 0.00010 + factor
            factor += 0.00001
            close = high
            self.vcr.update(high, low, close)

        # Assert
        self.assertEqual(0.9552015928322548, self.vcr.value, 2)

    def test_value_with_close_on_low_returns_expected_value(self):
        # Arrange
        high = 1.00010
        low = 1.00000
        factor = 0.0

        # Act
        for i in range(1000):
            high -= 0.00010 + factor
            low -= 0.00010 + factor
            factor -= 0.00002
            close = low
            self.vcr.update(high, low, close)

        # Assert
        self.assertEqual(0.9547511312217188, self.vcr.value)

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        for i in range(1000):
            self.vcr.update(1.00010, 1.00000, 1.00005)

        # Act
        self.vcr.reset()

        # Assert
        self.assertEqual(0.0, self.vcr.value)  # No assertion errors.

    def test_with_battery_signal(self):
        # Arrange
        battery_signal = BatterySeries.create()
        output = []

        # Act
        for point in BatterySeries.create():
            self.vcr.update(point, sys.float_info.epsilon, sys.float_info.epsilon)
            output.append(self.vcr.value)

        # Assert
        self.assertEqual(len(battery_signal), len(output))
