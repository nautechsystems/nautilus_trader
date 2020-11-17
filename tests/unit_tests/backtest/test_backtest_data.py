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

from nautilus_trader.backtest.data import BacktestDataContainer
from nautilus_trader.backtest.loaders import InstrumentLoader
from nautilus_trader.common.clock import TestClock
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import PriceType
from tests.test_kit.data_provider import TestDataProvider
from tests.test_kit.stubs import TestStubs


USDJPY_FXCM = TestStubs.symbol_usdjpy_fxcm()


class BacktestDataClientTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.usdjpy = InstrumentLoader.default_fx_ccy(TestStubs.symbol_usdjpy_fxcm())
        self.data = BacktestDataContainer()
        self.data.add_instrument(self.usdjpy)
        self.data.add_bars(self.usdjpy.symbol, BarAggregation.MINUTE, PriceType.BID, TestDataProvider.usdjpy_1min_bid()[:2000])
        self.data.add_bars(self.usdjpy.symbol, BarAggregation.MINUTE, PriceType.ASK, TestDataProvider.usdjpy_1min_ask()[:2000])
        self.test_clock = TestClock()
