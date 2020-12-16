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

from nautilus_trader.indicators.rsi import RelativeStrengthIndex
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


AUDUSD_FXCM = TestInstrumentProvider.default_fx_ccy(TestStubs.symbol_audusd_fxcm())


class RelativeStrengthIndexTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.rsi = RelativeStrengthIndex(10)

    def test_name_returns_expected_string(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual("RelativeStrengthIndex", self.rsi.name)

    def test_str_repr_returns_expected_string(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual("RelativeStrengthIndex(10, EXPONENTIAL)", str(self.rsi))
        self.assertEqual("RelativeStrengthIndex(10, EXPONENTIAL)", repr(self.rsi))

    def test_period_returns_expected_value(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual(10, self.rsi.period)

    def test_initialized_without_inputs_returns_false(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual(False, self.rsi.initialized)

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        self.rsi.update_raw(1.00000)
        self.rsi.update_raw(2.00000)
        self.rsi.update_raw(3.00000)
        self.rsi.update_raw(4.00000)
        self.rsi.update_raw(5.00000)
        self.rsi.update_raw(6.00000)
        self.rsi.update_raw(7.00000)
        self.rsi.update_raw(8.00000)
        self.rsi.update_raw(9.00000)
        self.rsi.update_raw(10.00000)

        # Act
        # Assert
        self.assertEqual(True, self.rsi.initialized)

    def test_handle_bar_updates_indicator(self):
        # Arrange
        indicator = RelativeStrengthIndex(10)

        bar = TestStubs.bar_5decimal()

        # Act
        indicator.handle_bar(bar)

        # Assert
        self.assertTrue(indicator.has_inputs)
        self.assertEqual(1.0, indicator.value)

    def test_value_with_one_input_returns_expected_value(self):
        # Arrange
        self.rsi.update_raw(1.00000)

        # Act
        # Assert
        self.assertEqual(1, self.rsi.value)

    def test_value_with_all_higher_inputs_returns_expected_value(self):
        # Arrange
        self.rsi.update_raw(1.00000)
        self.rsi.update_raw(2.00000)
        self.rsi.update_raw(3.00000)
        self.rsi.update_raw(4.00000)

        # Act
        # Assert
        self.assertEqual(1, self.rsi.value)

    def test_value_with_all_lower_inputs_returns_expected_value(self):
        # Arrange
        self.rsi.update_raw(3.00000)
        self.rsi.update_raw(2.00000)
        self.rsi.update_raw(1.00000)
        self.rsi.update_raw(0.50000)

        # Act
        # Assert
        self.assertEqual(0, self.rsi.value)

    def test_value_with_various_inputs_returns_expected_value(self):
        # Arrange
        self.rsi.update_raw(3.00000)
        self.rsi.update_raw(2.00000)
        self.rsi.update_raw(5.00000)
        self.rsi.update_raw(6.00000)
        self.rsi.update_raw(7.00000)
        self.rsi.update_raw(6.00000)

        # Act
        # Assert
        self.assertEqual(0.6837363325825265, self.rsi.value)

    def test_value_at_returns_expected_value(self):
        # Arrange
        self.rsi.update_raw(3.00000)
        self.rsi.update_raw(2.00000)
        self.rsi.update_raw(5.00000)
        self.rsi.update_raw(6.00000)
        self.rsi.update_raw(7.00000)
        self.rsi.update_raw(6.00000)
        self.rsi.update_raw(6.00000)
        self.rsi.update_raw(7.00000)

        # Act
        # Assert
        self.assertEqual(0.7615344667662725, self.rsi.value)

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        self.rsi.update_raw(1.00020)
        self.rsi.update_raw(1.00030)
        self.rsi.update_raw(1.00050)

        # Act
        self.rsi.reset()

        # Assert
        self.assertFalse(self.rsi.initialized)
        self.assertEqual(0, self.rsi.value)
