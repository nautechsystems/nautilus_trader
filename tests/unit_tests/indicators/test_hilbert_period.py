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

from nautilus_trader.indicators.hilbert_period import HilbertPeriod
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy(TestStubs.symbol_audusd_fxcm())


class HilbertPeriodTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.h_period = HilbertPeriod()

    def test_name_returns_expected_name(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual("HilbertPeriod", self.h_period.name)

    def test_str_returns_expected_string(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual("HilbertPeriod(7)", str(self.h_period))
        self.assertEqual("HilbertPeriod(7)", repr(self.h_period))

    def test_period_returns_expected_value(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual(7, self.h_period.period)

    def test_initialized_without_inputs_returns_false(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual(False, self.h_period.initialized)

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        # Act
        for _i in range(10):
            self.h_period.update_raw(1.00010, 1.00000)

        # Assert
        self.assertEqual(True, self.h_period.initialized)

    def test_handle_bar_updates_indicator(self):
        # Arrange
        indicator = HilbertPeriod()

        bar = TestStubs.bar_5decimal()

        # Act
        indicator.handle_bar(bar)

        # Assert
        self.assertTrue(indicator.has_inputs)
        self.assertEqual(0, indicator.value)

    def test_value_with_no_inputs_returns_none(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual(0, self.h_period.value)

    def test_value_with_epsilon_inputs_returns_expected_value(self):
        # Arrange
        for _i in range(100):
            self.h_period.update_raw(sys.float_info.epsilon, sys.float_info.epsilon)

        # Act
        # Assert
        self.assertEqual(7, self.h_period.value)

    def test_value_with_ones_inputs_returns_expected_value(self):
        # Arrange
        for _i in range(100):
            self.h_period.update_raw(1.00000, 1.00000)

        # Act
        # Assert
        self.assertEqual(7, self.h_period.value)

    def test_value_with_seven_inputs_returns_expected_value(self):
        # Arrange
        high = 1.00010
        low = 1.00000

        # Act
        for _i in range(9):
            high += 0.00010
            low += 0.00010
            self.h_period.update_raw(high, low)

        # Assert
        self.assertEqual(0, self.h_period.value)

    def test_value_with_close_on_high_returns_expected_value(self):
        # Arrange
        high = 1.00010
        low = 1.00000

        # Act
        for _i in range(1000):
            high += 0.00010
            low += 0.00010
            self.h_period.update_raw(high, low)

        # Assert
        self.assertEqual(7, self.h_period.value)

    def test_value_with_close_on_low_returns_expected_value(self):
        # Arrange
        high = 1.00010
        low = 1.00000

        # Act
        for _i in range(1000):
            high -= 0.00010
            low -= 0.00010
            self.h_period.update_raw(high, low)

        # Assert
        self.assertEqual(7, self.h_period.value)

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        for _i in range(1000):
            self.h_period.update_raw(1.00000, 1.00000)

        # Act
        self.h_period.reset()

        # Assert
        self.assertEqual(0, self.h_period.value)  # No exceptions raised
