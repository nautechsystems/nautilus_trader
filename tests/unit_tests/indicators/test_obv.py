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

from nautilus_trader.indicators.obv import OnBalanceVolume


class OnBalanceVolumeTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.obv = OnBalanceVolume(100)

    def test_name_returns_expected_string(self):
        # Act
        # Assert
        self.assertEqual("OnBalanceVolume", self.obv.name)

    def test_str_repr_returns_expected_string(self):
        # Act
        # Assert
        self.assertEqual("OnBalanceVolume(100)", str(self.obv))
        self.assertEqual("OnBalanceVolume(100)", repr(self.obv))

    def test_period_returns_expected_value(self):
        # Act
        # Assert
        self.assertEqual(100, self.obv.period)

    def test_initialized_without_inputs_returns_false(self):
        # Act
        # Assert
        self.assertEqual(False, self.obv.initialized)

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        for _i in range(100):
            self.obv.update_raw(1.00000, 1.00010, 10000)

        # Act
        # Assert
        self.assertEqual(True, self.obv.initialized)

    def test_value_with_one_input_returns_expected_value(self):
        # Arrange
        self.obv.update_raw(1.00000, 1.00010, 10000)

        # Act
        # Assert
        self.assertEqual(10000, self.obv.value)

    def test_values_with_higher_inputs_returns_expected_value(self):
        # Arrange
        self.obv.update_raw(1.00000, 1.00010, 10000)
        self.obv.update_raw(1.00000, 1.00010, 10000)
        self.obv.update_raw(1.00000, 1.00010, 10000)
        self.obv.update_raw(1.00000, 1.00010, 10000)
        self.obv.update_raw(1.00000, 1.00000, 10000)
        self.obv.update_raw(1.00000, 1.00010, 10000)
        self.obv.update_raw(1.00000, 1.00010, 10000)
        self.obv.update_raw(1.00000, 1.00010, 10000)
        self.obv.update_raw(1.00000, 1.00010, 10000)
        self.obv.update_raw(1.00000, 1.00010, 10000)

        # Act
        # Assert
        self.assertEqual(90000.0, self.obv.value)

    def test_values_with_lower_inputs_returns_expected_value(self):
        # Arrange
        self.obv.update_raw(1.00010, 1.00000, 10000)
        self.obv.update_raw(1.00010, 1.00000, 10000)
        self.obv.update_raw(1.00010, 1.00000, 10000)
        self.obv.update_raw(1.00010, 1.00000, 10000)
        self.obv.update_raw(1.00010, 1.00000, 10000)
        self.obv.update_raw(1.00010, 1.00000, 10000)
        self.obv.update_raw(1.00010, 1.00010, 10000)
        self.obv.update_raw(1.00010, 1.00000, 10000)
        self.obv.update_raw(1.00010, 1.00000, 10000)
        self.obv.update_raw(1.00010, 1.00000, 10000)

        # Act
        # Assert
        self.assertEqual(-90000.0, self.obv.value)

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        for _i in range(100):
            self.obv.update_raw(1.00000, 1.00010, 10000)

        # Act
        self.obv.reset()

        # Assert
        self.assertFalse(self.obv.initialized)
