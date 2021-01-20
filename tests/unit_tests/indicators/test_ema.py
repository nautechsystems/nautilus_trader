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

from nautilus_trader.indicators.average.ema import ExponentialMovingAverage
from nautilus_trader.model.enums import PriceType
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy(TestStubs.symbol_audusd())


class ExponentialMovingAverageTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.ema = ExponentialMovingAverage(10)

    def test_name_returns_expected_string(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual('ExponentialMovingAverage', self.ema.name)

    def test_str_repr_returns_expected_string(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual('ExponentialMovingAverage(10)', str(self.ema))
        self.assertEqual('ExponentialMovingAverage(10)', repr(self.ema))

    def test_period_returns_expected_value(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual(10, self.ema.period)

    def test_multiplier_returns_expected_value(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual(0.18181818181818182, self.ema.alpha)

    def test_initialized_without_inputs_returns_false(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual(False, self.ema.initialized)

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        self.ema.update_raw(1.00000)
        self.ema.update_raw(2.00000)
        self.ema.update_raw(3.00000)
        self.ema.update_raw(4.00000)
        self.ema.update_raw(5.00000)
        self.ema.update_raw(6.00000)
        self.ema.update_raw(7.00000)
        self.ema.update_raw(8.00000)
        self.ema.update_raw(9.00000)
        self.ema.update_raw(10.00000)

        # Act

        # Assert
        self.assertEqual(True, self.ema.initialized)

    def test_handle_quote_tick_updates_indicator(self):
        # Arrange
        indicator = ExponentialMovingAverage(10, PriceType.MID)

        tick = TestStubs.quote_tick_5decimal(AUDUSD_SIM.symbol)

        # Act
        indicator.handle_quote_tick(tick)

        # Assert
        self.assertTrue(indicator.has_inputs)
        self.assertEqual(1.00002, indicator.value)

    def test_handle_trade_tick_updates_indicator(self):
        # Arrange
        indicator = ExponentialMovingAverage(10)

        tick = TestStubs.trade_tick_5decimal(AUDUSD_SIM.symbol)

        # Act
        indicator.handle_trade_tick(tick)

        # Assert
        self.assertTrue(indicator.has_inputs)
        self.assertEqual(1.00001, indicator.value)

    def test_handle_bar_updates_indicator(self):
        # Arrange
        indicator = ExponentialMovingAverage(10)

        bar = TestStubs.bar_5decimal()

        # Act
        indicator.handle_bar(bar)

        # Assert
        self.assertTrue(indicator.has_inputs)
        self.assertEqual(1.00003, indicator.value)

    def test_value_with_one_input_returns_expected_value(self):
        # Arrange
        self.ema.update_raw(1.00000)

        # Act
        # Assert
        self.assertEqual(1.0, self.ema.value)

    def test_value_with_three_inputs_returns_expected_value(self):
        # Arrange
        self.ema.update_raw(1.00000)
        self.ema.update_raw(2.00000)
        self.ema.update_raw(3.00000)

        # Act
        # Assert
        self.assertEqual(1.5123966942148757, self.ema.value)

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        for _i in range(1000):
            self.ema.update_raw(1.00000)

        # Act
        self.ema.reset()

        # Assert
        self.assertFalse(self.ema.initialized)
        self.assertEqual(0.0, self.ema.value)
