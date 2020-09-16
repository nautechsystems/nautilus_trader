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

import time
import unittest

from nautilus_trader.indicators.average.sma import SimpleMovingAverage
from tests.test_kit.series import BatterySeries


class SimpleMovingAverageTests(unittest.TestCase):

    # Fixture Setup
    def setUp(self):
        # Arrange
        self.sma = SimpleMovingAverage(10)

    def test_name_returns_expected_name(self):
        # Act
        # Assert
        self.assertEqual("SimpleMovingAverage", self.sma.name)

    def test_str_returns_expected_string(self):
        # Act
        # Assert
        self.assertEqual("SimpleMovingAverage(10)", str(self.sma))

    def test_repr_returns_expected_string(self):
        # Act
        # Assert
        self.assertTrue(repr(self.sma).startswith("<SimpleMovingAverage(10) object at"))
        self.assertTrue(repr(self.sma).endswith(">"))

    def test_period_returns_expected_value(self):
        # Act
        # Assert
        self.assertEqual(10, self.sma.period)

    def test_initialized_without_inputs_returns_false(self):
        # Act
        # Assert
        self.assertEqual(False, self.sma.initialized)

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        self.sma.update_raw(1.00000)
        self.sma.update_raw(2.00000)
        self.sma.update_raw(3.00000)
        self.sma.update_raw(4.00000)
        self.sma.update_raw(5.00000)
        self.sma.update_raw(6.00000)
        self.sma.update_raw(7.00000)
        self.sma.update_raw(8.00000)
        self.sma.update_raw(9.00000)
        self.sma.update_raw(10.00000)

        # Act
        # Assert
        self.assertEqual(True, self.sma.initialized)
        self.assertEqual(10, self.sma.count)
        self.assertEqual(5.5, self.sma.value)

    def test_value_with_one_input_returns_expected_value(self):
        # Arrange
        self.sma.update_raw(1.00000)

        # Act
        # Assert
        self.assertEqual(1.0, self.sma.value)

    def test_value_with_three_inputs_returns_expected_value(self):
        # Arrange
        self.sma.update_raw(1.00000)
        self.sma.update_raw(2.00000)
        self.sma.update_raw(3.00000)

        # Act
        # Assert
        self.assertEqual(2.0, self.sma.value)

    def test_value_at_returns_expected_value(self):
        # Arrange
        self.sma.update_raw(1.00000)
        self.sma.update_raw(2.00000)
        self.sma.update_raw(3.00000)

        # Act
        # Assert
        self.assertEqual(2.0, self.sma.value)

    def test_with_battery_signal(self):
        # Arrange
        tt = time.time()
        battery_signal = BatterySeries.create(length=1000000)
        output = []

        # Act
        for point in battery_signal:
            self.sma.update_raw(point)
            output.append(self.sma.value)

        # Assert
        self.assertEqual(len(battery_signal), len(output))
        print(self.sma.value)
        print(time.time() - tt)
