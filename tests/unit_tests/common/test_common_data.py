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

from nautilus_trader.backtest.clock import TestClock
from nautilus_trader.backtest.logging import TestLogger
from nautilus_trader.backtest.uuid import TestUUIDFactory
from nautilus_trader.common.data_engine import DataEngine
from nautilus_trader.model.enums import Currency
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.tick import QuoteTick
from tests.test_kit.stubs import TestStubs

AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()
USDJPY_FXCM = TestStubs.symbol_usdjpy_fxcm()


class DataClientTests(unittest.TestCase):

    def setUp(self):
        clock = TestClock()
        self.data_engine = DataEngine(
            tick_capacity=1000,
            bar_capacity=1000,
            clock=clock,
            uuid_factory=TestUUIDFactory(),
            logger=TestLogger(clock),
        )

    def test_get_exchange_rate_returns_correct_rate(self):
        # Arrange
        tick = QuoteTick(
            USDJPY_FXCM,
            Price(110.80000, 5),
            Price(110.80010, 5),
            Quantity(1),
            Quantity(1),
            datetime(2018, 1, 1, 19, 59, 1, 0, pytz.utc),
        )

        self.data_engine.handle_quote_tick(tick)

        # Act
        result = self.data_engine.get_exchange_rate(Currency.JPY, Currency.USD)

        # Assert
        self.assertEqual(0.009025266685348969, result)

    def test_can_get_exchange_rate_with_no_conversion(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_FXCM,
            Price(0.80000, 5),
            Price(0.80010, 5),
            Quantity(1),
            Quantity(1),
            datetime(2018, 1, 1, 19, 59, 1, 0, pytz.utc),
        )

        self.data_engine.handle_quote_tick(tick)

        # Act
        result = self.data_engine.get_exchange_rate(Currency.AUD, Currency.USD)

        # Assert
        self.assertEqual(0.80005, result)
