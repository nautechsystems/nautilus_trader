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

from nautilus_trader.backtest.loaders import InstrumentLoader
from nautilus_trader.indicators.bollinger_bands import BollingerBands
from tests.test_kit.stubs import TestStubs


AUDUSD_FXCM = InstrumentLoader.default_fx_ccy(TestStubs.symbol_audusd_fxcm())


class BollingerBandsTests(unittest.TestCase):

    def test_name_returns_expected_name(self):
        # Arrange
        indicator = BollingerBands(20, 2.0)

        # Act
        # Assert
        self.assertEqual("BollingerBands", indicator.name)

    def test_str_repr_returns_expected_string(self):
        # Arrange
        indicator = BollingerBands(20, 2.0)

        # Act
        # Assert
        self.assertEqual("BollingerBands(20, 2.0, SIMPLE)", str(indicator))
        self.assertEqual("BollingerBands(20, 2.0, SIMPLE)", repr(indicator))

    def test_properties_after_instantiation(self):
        # Arrange
        indicator = BollingerBands(20, 2.0)

        # Act
        # Assert
        self.assertEqual(20, indicator.period)
        self.assertEqual(2.0, indicator.k)
        self.assertEqual(0, indicator.upper)
        self.assertEqual(0, indicator.lower)
        self.assertEqual(0, indicator.middle)

    def test_initialized_with_required_inputs_returns_true(self):
        # Arrange
        indicator = BollingerBands(5, 2.0)

        indicator.update_raw(1.00000, 1.00000, 1.00000)
        indicator.update_raw(1.00000, 1.00000, 1.00000)
        indicator.update_raw(1.00000, 1.00000, 1.00000)
        indicator.update_raw(1.00000, 1.00000, 1.00000)
        indicator.update_raw(1.00000, 1.00000, 1.00000)

        # Act
        # Assert
        self.assertEqual(True, indicator.initialized)

    def test_handle_quote_tick_updates_indicator(self):
        # Arrange
        indicator = BollingerBands(20, 2.0)

        tick = TestStubs.quote_tick_5decimal(AUDUSD_FXCM.symbol)

        # Act
        indicator.handle_quote_tick(tick)

        # Assert
        self.assertTrue(indicator.has_inputs)
        self.assertEqual(1.1666916666666667, indicator.middle)

    def test_handle_trade_tick_updates_indicator(self):
        # Arrange
        indicator = BollingerBands(20, 2.0)

        tick = TestStubs.trade_tick_5decimal(AUDUSD_FXCM.symbol)

        # Act
        indicator.handle_trade_tick(tick)

        # Assert
        self.assertTrue(indicator.has_inputs)
        self.assertEqual(1.00001, indicator.middle)

    def test_handle_bar_updates_indicator(self):
        # Arrange
        indicator = BollingerBands(20, 2.0)

        bar = TestStubs.bar_5decimal()

        # Act
        indicator.handle_bar(bar)

        # Assert
        self.assertTrue(indicator.has_inputs)
        self.assertEqual(1.0000266666666666, indicator.middle)

    def test_value_with_one_input_returns_expected_value(self):
        # Arrange
        indicator = BollingerBands(20, 2.0)

        # Act
        indicator.update_raw(1.00020, 1.00000, 1.00010)

        # Assert
        self.assertEqual(1.00010, indicator.upper)
        self.assertEqual(1.00010, indicator.middle)
        self.assertEqual(1.00010, indicator.lower)

    def test_value_with_three_inputs_returns_expected_value(self):
        # Arrange
        indicator = BollingerBands(20, 2.0)

        # Act
        indicator.update_raw(1.00020, 1.00000, 1.00015)
        indicator.update_raw(1.00030, 1.00010, 1.00015)
        indicator.update_raw(1.00040, 1.00020, 1.00021)

        # Assert
        self.assertEqual(1.0003155506390384, indicator.upper)
        self.assertEqual(1.0001900000000001, indicator.middle)
        self.assertEqual(1.0000644493609618, indicator.lower)

    def test_reset_successfully_returns_indicator_to_fresh_state(self):
        # Arrange
        indicator = BollingerBands(5, 2.0)

        indicator.update_raw(1.00000, 1.00000, 1.00000)
        indicator.update_raw(1.00000, 1.00000, 1.00000)
        indicator.update_raw(1.00000, 1.00000, 1.00000)
        indicator.update_raw(1.00000, 1.00000, 1.00000)
        indicator.update_raw(1.00000, 1.00000, 1.00000)

        # Act
        indicator.reset()

        # Assert
        self.assertFalse(indicator.initialized)
        self.assertEqual(0, indicator.upper)
        self.assertEqual(0, indicator.middle)
        self.assertEqual(0, indicator.lower)
