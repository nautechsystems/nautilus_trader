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

from datetime import datetime
import unittest

import pytz

from nautilus_trader.backtest.logging import TestLogger
from nautilus_trader.common.clock import TestClock
from nautilus_trader.data.cache import DataCache
from nautilus_trader.model.bar import Bar
from nautilus_trader.model.enums import Maker
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeMatchId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.model.tick import TradeTick
from tests.test_kit.stubs import TestStubs


USDJPY_FXCM = Symbol('USD/JPY', Venue('FXCM'))
AUDUSD_FXCM = Symbol('AUD/USD', Venue('FXCM'))


class DataCacheTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup

        self.cache = DataCache(logger=TestLogger(TestClock()))

    def test_get_tick_count_for_unknown_symbol_returns_zero(self):
        # Arrange
        # Act
        result = self.cache.quote_tick_count(AUDUSD_FXCM)

        # Assert
        self.assertEqual(0, result)

    def test_get_ticks_for_unknown_symbol_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(KeyError, self.cache.quote_ticks, AUDUSD_FXCM)

    def test_get_bar_count_for_unknown_bar_type_returns_zero(self):
        # Arrange
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()

        # Act
        result = self.cache.bar_count(bar_type)

        # Assert
        self.assertEqual(0, result)

    def test_get_bars_for_unknown_bar_type_raises_exception(self):
        # Arrange
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()

        # Act
        # Assert
        self.assertRaises(KeyError, self.cache.bars, bar_type)

    def test_bars(self):
        # Arrange
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        bar = Bar(
            Price("1.00001"),
            Price("1.00004"),
            Price("1.00002"),
            Price("1.00003"),
            Quantity(100000),
            datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc),
        )

        self.cache.add_bar(bar_type, bar)

        # Act
        result = self.cache.bars(bar_type)

        # Assert
        self.assertTrue(bar, result[0])

    def test_getting_bar_for_unknown_bar_type_raises_exception(self):
        # Arrange
        unknown_bar_type = TestStubs.bartype_gbpusd_1sec_mid()

        # Act
        # Assert
        self.assertRaises(KeyError, self.cache.bar, unknown_bar_type, 0)

    def test_getting_bar_at_out_of_range_index_raises_exception(self):
        # Arrange
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        bar = Bar(
            Price("1.00001"),
            Price("1.00004"),
            Price("1.00002"),
            Price("1.00003"),
            Quantity(100000),
            datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc),
        )

        self.cache.add_bar(bar_type, bar)

        # Act
        # Assert
        self.assertRaises(IndexError, self.cache.bar, bar_type, -2)

    def test_get_bar(self):
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        bar = Bar(
            Price("1.00001"),
            Price("1.00004"),
            Price("1.00002"),
            Price("1.00003"),
            Quantity(100000),
            datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc),
        )

        self.cache.add_bar(bar_type, bar)

        # Act
        result = self.cache.bar(bar_type, 0)

        # Assert
        self.assertEqual(bar, result)

    def test_getting_tick_with_unknown_tick_type_raises_exception(self):
        # Act
        # Assert
        self.assertRaises(KeyError, self.cache.quote_tick, AUDUSD_FXCM, 0)

    def test_get_quote_tick(self):
        tick = QuoteTick(
            AUDUSD_FXCM,
            Price("1.00000"),
            Price("1.00001"),
            Quantity(1),
            Quantity(1),
            datetime(2018, 1, 1, 19, 59, 1, 0, pytz.utc),
        )

        self.cache.add_quote_tick(tick)

        # Act
        result = self.cache.quote_tick(tick.symbol, 0)

        # Assert
        self.assertEqual(tick, result)

    def test_get_trade_tick(self):
        tick = TradeTick(
            AUDUSD_FXCM,
            Price("1.00000"),
            Quantity(10000),
            Maker.BUYER,
            TradeMatchId("123456789"),
            datetime(2018, 1, 1, 19, 59, 1, 0, pytz.utc),
        )

        self.cache.add_trade_tick(tick)

        # Act
        result = self.cache.trade_tick(tick.symbol, 0)

        # Assert
        self.assertEqual(tick, result)
