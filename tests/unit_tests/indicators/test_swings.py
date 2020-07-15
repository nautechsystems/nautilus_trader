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

from nautilus_trader.indicators.swings import Swings
from tests.test_kit.series import BatterySeries
from tests.test_kit.stubs import UNIX_EPOCH


class SwingsTests(unittest.TestCase):

    # test fixture
    def setUp(self):
        # arrange
        self.swings = Swings(3)

    def test_name_returns_expected_name(self):
        # act
        # assert
        self.assertEqual('Swings', self.swings.name)

    def test_str_returns_expected_string(self):
        # act
        # assert
        self.assertEqual('Swings(3)', str(self.swings))

    def test_repr_returns_expected_string(self):
        # act
        # assert
        self.assertTrue(repr(self.swings).startswith('<Swings(3) object at'))

    def test_period_returns_expected_value(self):
        # act
        # assert
        self.assertEqual(3, self.swings.period)

    def test_properties_with_no_values_returns_expected(self):
        # act
        # assert
        self.assertEqual(False, self.swings.initialized)
        self.assertEqual(0, self.swings.direction)
        self.assertEqual(0, self.swings.value)
        self.assertEqual(False, self.swings.changed)
        self.assertEqual(0, self.swings.since_high)
        self.assertEqual(0, self.swings.since_low)

    def test_can_determine_swing_high(self):
        # arrange
        self.swings.update(1.00010, 1.00000, UNIX_EPOCH)
        self.swings.update(1.00030, 1.00010, UNIX_EPOCH)
        self.swings.update(1.00040, 1.00020, UNIX_EPOCH)
        self.swings.update(1.00050, 1.00030, UNIX_EPOCH)
        self.swings.update(1.00060, 1.00040, UNIX_EPOCH)
        self.swings.update(1.00050, 1.00040, UNIX_EPOCH)

        # act
        result = self.swings.high_price

        # assert
        self.assertEqual(1, self.swings.direction)
        self.assertEqual(1.0006, result)

    def test_can_determine_swing_low(self):
        # arrange
        self.swings.update(1.00100, 1.00080, UNIX_EPOCH)
        self.swings.update(1.00080, 1.00060, UNIX_EPOCH)
        self.swings.update(1.00060, 1.00040, UNIX_EPOCH)
        self.swings.update(1.00040, 1.00030, UNIX_EPOCH)
        self.swings.update(1.00020, 1.00010, UNIX_EPOCH)
        self.swings.update(1.00020, 1.00020, UNIX_EPOCH)

        # act
        result = self.swings.low_price

        # assert
        self.assertEqual(-1, self.swings.direction)
        self.assertEqual(1.0001, result)

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        battery_signal = BatterySeries.create()

        for point in battery_signal:
            self.swings.update(point, point, UNIX_EPOCH)

        # Act
        self.swings.reset()

        # Assert
        self.assertEqual(0, self.swings.value)  # No assertion errors.

    def test_reset_inputs_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        battery_signal = BatterySeries.create()

        for point in battery_signal:
            self.swings.update(point, point, UNIX_EPOCH)

        # Act
        self.swings.reset()

        # Assert
        self.assertEqual(0, self.swings.value)  # No assertion errors.

    def test_with_battery_signal(self):
        # Arrange
        battery_signal = BatterySeries.create()
        output = []

        # Act
        for point in battery_signal:
            self.swings.update(point + 0.00010, point, UNIX_EPOCH)
            output.append(self.swings.value)

        # Assert
        self.assertEqual(len(battery_signal), len(output))
