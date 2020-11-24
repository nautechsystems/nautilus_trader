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

from nautilus_trader.indicators.donchian_channel import DonchianChannel


class DonchianChannelTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.dc = DonchianChannel(10)

    def test_name_returns_expected_name(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual("DonchianChannel", self.dc.name)

    def test_str_repr_returns_expected_string(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual("DonchianChannel(10)", str(self.dc))
        self.assertEqual("DonchianChannel(10)", repr(self.dc))

    def test_period_returns_expected_value(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual(10, self.dc.period)

    def test_initialized_without_inputs_returns_false(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual(False, self.dc.initialized)

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        self.dc.update_raw(1.00000, 1.00000)
        self.dc.update_raw(1.00000, 1.00000)
        self.dc.update_raw(1.00000, 1.00000)
        self.dc.update_raw(1.00000, 1.00000)
        self.dc.update_raw(1.00000, 1.00000)
        self.dc.update_raw(1.00000, 1.00000)
        self.dc.update_raw(1.00000, 1.00000)
        self.dc.update_raw(1.00000, 1.00000)
        self.dc.update_raw(1.00000, 1.00000)
        self.dc.update_raw(1.00000, 1.00000)

        # Act
        # Assert
        self.assertEqual(True, self.dc.initialized)

    def test_value_with_one_input_returns_expected_value(self):
        # Arrange
        self.dc.update_raw(1.00020, 1.00000)

        # Act
        # Assert
        self.assertEqual(1.00020, self.dc.upper)
        self.assertEqual(1.00010, self.dc.middle)
        self.assertEqual(1.00000, self.dc.lower)

    def test_value_with_three_inputs_returns_expected_value(self):
        # Arrange
        self.dc.update_raw(1.00020, 1.00000)
        self.dc.update_raw(1.00030, 1.00010)
        self.dc.update_raw(1.00040, 1.00020)

        # Act
        # Assert
        self.assertEqual(1.00040, self.dc.upper)
        self.assertEqual(1.00020, self.dc.middle)
        self.assertEqual(1.00000, self.dc.lower)

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        self.dc.update_raw(1.00020, 1.00000)
        self.dc.update_raw(1.00030, 1.00010)
        self.dc.update_raw(1.00040, 1.00020)

        # Act
        self.dc.reset()

        # Assert
        self.assertFalse(self.dc.initialized)
        self.assertEqual(0, self.dc.upper)
        self.assertEqual(0, self.dc.middle)
        self.assertEqual(0, self.dc.lower)
