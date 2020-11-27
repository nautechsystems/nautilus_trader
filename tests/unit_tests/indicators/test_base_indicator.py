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
from nautilus_trader.indicators.base.indicator import Indicator
from tests.test_kit.stubs import TestStubs


AUDUSD_FXCM = InstrumentLoader.default_fx_ccy(TestStubs.symbol_audusd_fxcm())


class IndicatorTests(unittest.TestCase):

    def test_handle_quote_tick_raises_not_implemented_error(self):
        # Arrange
        indicator = Indicator([])

        tick = TestStubs.quote_tick_5decimal(AUDUSD_FXCM.symbol)

        # Act
        # Assert
        self.assertRaises(NotImplementedError, indicator.handle_quote_tick, tick)

    def test_handle_trade_tick_raises_not_implemented_error(self):
        # Arrange
        indicator = Indicator([])

        tick = TestStubs.trade_tick_5decimal(AUDUSD_FXCM.symbol)

        # Act
        # Assert
        self.assertRaises(NotImplementedError, indicator.handle_trade_tick, tick)

    def test_handle_bar_raises_not_implemented_error(self):
        # Arrange
        indicator = Indicator([])

        bar = TestStubs.bar_5decimal()

        # Act
        # Assert
        self.assertRaises(NotImplementedError, indicator.handle_bar, bar)

    def test_reset_raises_not_implemented_error(self):
        # Arrange
        indicator = Indicator([])

        # Act
        # Assert
        self.assertRaises(NotImplementedError, indicator.reset)
