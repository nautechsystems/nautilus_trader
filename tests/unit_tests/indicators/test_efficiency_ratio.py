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

from nautilus_trader.indicators.efficiency_ratio import EfficiencyRatio
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


AUDUSD_FXCM = TestInstrumentProvider.default_fx_ccy(TestStubs.symbol_audusd_fxcm())


class EfficiencyRatioTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.er = EfficiencyRatio(10)

    def test_name_returns_expected_string(self):
        # Act
        # Assert
        self.assertEqual("EfficiencyRatio", self.er.name)

    def test_str_repr_returns_expected_string(self):
        # Act
        # Assert
        self.assertEqual("EfficiencyRatio(10)", str(self.er))
        self.assertEqual("EfficiencyRatio(10)", repr(self.er))

    def test_period(self):
        # Act
        # Assert
        self.assertEqual(10, self.er.period)

    def test_initialized_without_inputs_returns_false(self):
        # Act
        # Assert
        self.assertEqual(False, self.er.initialized)

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        # Act
        for _i in range(10):
            self.er.update_raw(1.00000)

        # Assert
        self.assertEqual(True, self.er.initialized)

    def test_handle_bar_updates_indicator(self):
        # Arrange
        indicator = EfficiencyRatio(10)

        bar = TestStubs.bar_5decimal()

        # Act
        indicator.handle_bar(bar)

        # Assert
        self.assertTrue(indicator.has_inputs)
        self.assertEqual(0, indicator.value)

    def test_value_with_one_input(self):
        # Arrange
        self.er.update_raw(1.00000)

        # Act
        # Assert
        self.assertEqual(0.0, self.er.value)

    def test_value_with_efficient_higher_inputs(self):
        # Arrange
        initial_price = 1.00000

        # Act
        for _i in range(10):
            initial_price += 0.00001
            self.er.update_raw(initial_price)

        # Assert
        self.assertEqual(1.0, self.er.value)

    def test_value_with_efficient_lower_inputs(self):
        # Arrange
        initial_price = 1.00000

        # Act
        for _i in range(10):
            initial_price -= 0.00001
            self.er.update_raw(initial_price)

        # Assert
        self.assertEqual(1.0, self.er.value)

    def test_value_with_oscillating_inputs_returns_zero(self):
        # Arrange
        self.er.update_raw(1.00000)
        self.er.update_raw(1.00010)
        self.er.update_raw(1.00000)
        self.er.update_raw(0.99990)
        self.er.update_raw(1.00000)

        # Act
        # Assert
        self.assertEqual(0.0, self.er.value)

    def test_value_with_half_oscillating_inputs_returns_zero(self):
        # Arrange
        self.er.update_raw(1.00000)
        self.er.update_raw(1.00020)
        self.er.update_raw(1.00010)
        self.er.update_raw(1.00030)
        self.er.update_raw(1.00020)

        # Act
        # Assert
        self.assertEqual(0.3333333333333333, self.er.value)

    def test_value_with_noisy_inputs(self):
        # Arrange
        self.er.update_raw(1.00000)
        self.er.update_raw(1.00010)
        self.er.update_raw(1.00008)
        self.er.update_raw(1.00007)
        self.er.update_raw(1.00012)
        self.er.update_raw(1.00005)
        self.er.update_raw(1.00015)

        # Act
        # Assert
        self.assertEqual(0.42857142857215363, self.er.value)

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        for _i in range(10):
            self.er.update_raw(1.00000)

        # Act
        self.er.reset()

        # Assert
        self.assertFalse(self.er.initialized)
        self.assertEqual(0, self.er.value)
