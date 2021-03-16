# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.model.bar import Bar
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.test_kit.stubs import UNIX_EPOCH


class SwingsTests(unittest.TestCase):
    def setUp(self):
        # Fixture Setup
        self.swings = Swings(3)

    def test_name_returns_expected_name(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual("Swings", self.swings.name)

    def test_str_repr_returns_expected_string(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual("Swings(3)", str(self.swings))
        self.assertEqual("Swings(3)", repr(self.swings))

    def test_instantiate_returns_expected_property_values(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual(3, self.swings.period)
        self.assertEqual(False, self.swings.initialized)
        self.assertEqual(0, self.swings.direction)
        self.assertEqual(False, self.swings.changed)
        self.assertEqual(0, self.swings.since_high)
        self.assertEqual(0, self.swings.since_low)

    def test_handle_bar(self):
        # Arrange
        bar = Bar(
            Price("1.00000"),
            Price("1.00004"),
            Price("1.00002"),
            Price("1.00003"),
            Quantity(100000),
            UNIX_EPOCH,
        )

        # Act
        self.swings.handle_bar(bar)

        # Assert
        self.assertTrue(self.swings.has_inputs)

    def test_determine_swing_high(self):
        # Arrange
        self.swings.update_raw(1.00010, 1.00000, UNIX_EPOCH)
        self.swings.update_raw(1.00030, 1.00010, UNIX_EPOCH)
        self.swings.update_raw(1.00040, 1.00020, UNIX_EPOCH)
        self.swings.update_raw(1.00050, 1.00030, UNIX_EPOCH)
        self.swings.update_raw(1.00060, 1.00040, UNIX_EPOCH)
        self.swings.update_raw(1.00050, 1.00040, UNIX_EPOCH)

        # Act
        # Assert
        self.assertEqual(1, self.swings.direction)
        self.assertEqual(1.0006, self.swings.high_price)

    def test_determine_swing_low(self):
        # Arrange
        self.swings.update_raw(1.00100, 1.00080, UNIX_EPOCH)
        self.swings.update_raw(1.00080, 1.00060, UNIX_EPOCH)
        self.swings.update_raw(1.00060, 1.00040, UNIX_EPOCH)
        self.swings.update_raw(1.00040, 1.00030, UNIX_EPOCH)
        self.swings.update_raw(1.00020, 1.00010, UNIX_EPOCH)
        self.swings.update_raw(1.00020, 1.00020, UNIX_EPOCH)

        # Act
        # Assert
        self.assertEqual(-1, self.swings.direction)
        self.assertEqual(1.0001, self.swings.low_price)

    def test_swing_change_high_to_low(self):
        # Arrange
        self.swings.update_raw(1.00010, 1.00000, UNIX_EPOCH)
        self.swings.update_raw(1.00020, 1.00010, UNIX_EPOCH)
        self.swings.update_raw(1.00030, 1.00020, UNIX_EPOCH)
        self.swings.update_raw(1.00040, 1.00030, UNIX_EPOCH)
        self.swings.update_raw(1.00050, 1.00040, UNIX_EPOCH)
        self.swings.update_raw(1.00060, 1.00050, UNIX_EPOCH)
        self.swings.update_raw(1.00050, 1.00040, UNIX_EPOCH)

        # Act
        # Assert
        self.assertEqual(-1, self.swings.direction)
        self.assertTrue(self.swings.changed)
        self.assertEqual(0, self.swings.since_low)
        self.assertEqual(1, self.swings.since_high)
        self.assertEqual(0, self.swings.length)  # Just changed

    def test_swing_change_low_to_high(self):
        # Arrange
        self.swings.update_raw(1.00090, 1.00080, UNIX_EPOCH)
        self.swings.update_raw(1.00080, 1.00070, UNIX_EPOCH)
        self.swings.update_raw(1.00070, 1.00060, UNIX_EPOCH)
        self.swings.update_raw(1.00060, 1.00050, UNIX_EPOCH)
        self.swings.update_raw(1.00050, 1.00040, UNIX_EPOCH)
        self.swings.update_raw(1.00060, 1.00050, UNIX_EPOCH)

        # Act
        # Assert
        self.assertEqual(1, self.swings.direction)
        self.assertTrue(self.swings.changed)
        self.assertEqual(0, self.swings.since_high)
        self.assertEqual(1, self.swings.since_low)
        self.assertEqual(0, self.swings.length)  # Just changed

    def test_swing_changes(self):
        # Arrange
        self.swings.update_raw(1.00010, 1.00000, UNIX_EPOCH)
        self.swings.update_raw(1.00020, 1.00010, UNIX_EPOCH)
        self.swings.update_raw(1.00030, 1.00020, UNIX_EPOCH)
        self.swings.update_raw(1.00040, 1.00030, UNIX_EPOCH)
        self.swings.update_raw(1.00050, 1.00040, UNIX_EPOCH)
        self.swings.update_raw(1.00060, 1.00050, UNIX_EPOCH)
        self.swings.update_raw(1.00050, 1.00040, UNIX_EPOCH)
        self.swings.update_raw(1.00040, 1.00030, UNIX_EPOCH)
        self.swings.update_raw(1.00030, 1.00020, UNIX_EPOCH)
        self.swings.update_raw(1.00020, 1.00010, UNIX_EPOCH)
        self.swings.update_raw(1.00010, 1.00000, UNIX_EPOCH)
        self.swings.update_raw(1.00020, 1.00010, UNIX_EPOCH)
        self.swings.update_raw(1.00030, 1.00020, UNIX_EPOCH)
        self.swings.update_raw(1.00040, 1.00030, UNIX_EPOCH)

        # Act
        # Assert
        self.assertEqual(1, self.swings.direction)
        self.assertEqual(3, self.swings.since_low)
        self.assertEqual(0, self.swings.since_high)
        self.assertEqual(0.00039999999999995595, self.swings.length)
        self.assertTrue(self.swings.initialized)

    def test_reset(self):
        # Arrange
        self.swings.update_raw(1.00100, 1.00080, UNIX_EPOCH)
        self.swings.update_raw(1.00080, 1.00060, UNIX_EPOCH)
        self.swings.update_raw(1.00060, 1.00040, UNIX_EPOCH)

        # Act
        self.swings.reset()

        # Assert
        self.assertEqual(0, self.swings.has_inputs)
        self.assertEqual(0, self.swings.direction)
