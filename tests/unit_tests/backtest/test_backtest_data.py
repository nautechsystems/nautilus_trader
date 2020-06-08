# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import unittest
from pandas import Timestamp

from nautilus_trader.model.enums import BarStructure, PriceType
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import TestLogger
from nautilus_trader.backtest.data import BacktestDataContainer, BacktestDataClient

from tests.test_kit.data import TestDataProvider
from tests.test_kit.stubs import TestStubs

USDJPY_FXCM = TestStubs.instrument_usdjpy().symbol


class BacktestDataClientTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.usdjpy = TestStubs.instrument_usdjpy()
        self.data = BacktestDataContainer()
        self.data.add_instrument(self.usdjpy)
        self.data.add_bars(self.usdjpy.symbol, BarStructure.MINUTE, PriceType.BID, TestDataProvider.usdjpy_1min_bid()[:2000])
        self.data.add_bars(self.usdjpy.symbol, BarStructure.MINUTE, PriceType.ASK, TestDataProvider.usdjpy_1min_ask()[:2000])
        self.test_clock = TestClock()

    def test_can_initialize_client_with_data(self):
        # Arrange
        client = BacktestDataClient(
            data=self.data,
            tick_capacity=100,
            clock=self.test_clock,
            logger=TestLogger())

        # Act
        # Assert
        self.assertEqual(Timestamp('2013-01-01 21:59:59.900000+0000', tz='UTC'), client.min_timestamp)
        self.assertEqual(Timestamp('2013-01-02 09:19:00+0000', tz='UTC'), client.max_timestamp)
