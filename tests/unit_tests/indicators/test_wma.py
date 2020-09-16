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

from nautilus_trader.indicators.average.ma_factory import MovingAverageFactory
from nautilus_trader.indicators.average.moving_average import MovingAverageType
from nautilus_trader.indicators.average.wma import WeightedMovingAverage
from tests.test_kit.series import BatterySeries


class WeightedMovingAverageTests(unittest.TestCase):

    # Fixture Setup
    def setUp(self):
        # Arrange
        self.w = [round(i * 0.1, 2) for i in range(1, 11)]
        self.wma = WeightedMovingAverage(10, self.w)
        self.wma_noweights = WeightedMovingAverage(10)
        self.wma_factory = MovingAverageFactory.create(10, MovingAverageType.WEIGHTED, weights=self.w)

    def test_name_returns_expected_name(self):
        # Act
        # Assert
        self.assertEqual("WeightedMovingAverage", self.wma.name)

    def test_str_returns_expected_string(self):
        # Act
        # Assert
        self.assertEqual(
            "WeightedMovingAverage(10, [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0])",
            str(self.wma))

    def test_repr_returns_expected_string(self):
        # Act
        # Assert
        self.assertTrue(repr(self.wma).startswith(
            "<WeightedMovingAverage(10, [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0]) object at"))
        self.assertTrue(repr(self.wma).endswith(">"))

    def test_weights_returns_expected_weights(self):
        # Act
        # Assert
        self.assertEqual(self.w, self.wma.weights)

    def test_wma_factory_kwargs(self):
        for i in range(1, 12):
            self.wma_factory.update_raw(float(i))

        self.assertEqual(8.0, self.wma_factory.value)
        self.assertEqual(self.w, self.wma_factory.weights)

    def test_value_with_one_input_returns_expected_value(self):
        # Arrange
        self.wma.update_raw(1.00000)

        # Act
        # Assert
        self.assertEqual(1.0, self.wma.value)

    def test_value_with_two_input_returns_expected_value(self):
        # Arrange
        self.wma.update_raw(1.00000)
        self.wma.update_raw(10.00000)

        # 10 * 1.0, 1 * 0.9

        # Act
        # Assert
        self.assertEqual((10 * 1.0 + 1 * 0.9) / 1.9, self.wma.value)

    def test_value_with_no_weights(self):
        # Arrange
        self.wma_noweights.update_raw(1.00000)
        self.wma_noweights.update_raw(2.00000)

        # Act
        # Assert
        self.assertEqual(1.5, self.wma_noweights.value)

    def test_value_with_ten_inputs_returns_expected_value(self):
        # Arrange
        self.wma.update_raw(1.00000)
        self.wma.update_raw(2.00000)
        self.wma.update_raw(3.00000)
        self.wma.update_raw(4.00000)
        self.wma.update_raw(5.00000)
        self.wma.update_raw(6.00000)
        self.wma.update_raw(7.00000)
        self.wma.update_raw(8.00000)
        self.wma.update_raw(9.00000)
        self.wma.update_raw(10.00000)

        # Act
        # Assert
        self.assertAlmostEqual(7.00, self.wma.value, 2)

    def test_value_at_returns_expected_value(self):
        # Arrange
        self.wma.update_raw(1.00000)
        self.wma.update_raw(2.00000)
        self.wma.update_raw(3.00000)
        self.wma.update_raw(4.00000)
        self.wma.update_raw(5.00000)
        self.wma.update_raw(6.00000)
        self.wma.update_raw(7.00000)
        self.wma.update_raw(8.00000)
        self.wma.update_raw(9.00000)
        self.wma.update_raw(10.00000)
        self.wma.update_raw(11.00000)

        # Act
        # Assert
        self.assertEqual(8.0, self.wma.value)

    def test_with_battery_signal(self):
        # Arrange
        battery_signal = BatterySeries.create()
        output = []

        # Act
        for point in battery_signal:
            self.wma.update_raw(point)
            output.append(self.wma.value)

        # Assert
        self.assertEqual(len(battery_signal), len(output))
