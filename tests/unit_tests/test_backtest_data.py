#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_backtest_data.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import pandas as pd
import unittest

from datetime import datetime, timezone, timedelta

from inv_trader.common.clock import TestClock
from inv_trader.common.logger import Logger
from inv_trader.model.enums import Resolution
from inv_trader.model.enums import Venue, OrderSide
from inv_trader.model.identifiers import Label, OrderId, PositionId
from inv_trader.model.objects import Symbol
from inv_trader.model.events import OrderRejected, OrderWorking, OrderModified, OrderFilled
from inv_trader.backtest.data import BacktestDataClient
from inv_trader.backtest.execution import BacktestExecClient
from inv_trader.backtest.engine import BacktestConfig, BacktestEngine
from test_kit.objects import ObjectStorer
from test_kit.strategies import EmptyStrategy, TestStrategy1, EMACross
from test_kit.data import TestDataProvider
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
USDJPY_FXCM = Symbol('USDJPY', Venue.FXCM)


class BacktestDataClientTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.usdjpy = TestStubs.instrument_usdjpy()
        self.bid_data_1min = TestDataProvider.usdjpy_1min_bid().iloc[:2000]
        self.ask_data_1min = TestDataProvider.usdjpy_1min_ask().iloc[:2000]

        self.instruments = [TestStubs.instrument_usdjpy()]
        self.data_ticks = {self.usdjpy.symbol: pd.DataFrame()}
        self.data_bars_bid = {self.usdjpy.symbol: {Resolution.MINUTE: self.bid_data_1min}}
        self.data_bars_ask = {self.usdjpy.symbol: {Resolution.MINUTE: self.ask_data_1min}}

        self.test_clock = TestClock()
        self.client = BacktestDataClient(
            instruments=self.instruments,
            data_ticks=self.data_ticks,
            data_bars_bid=self.data_bars_bid,
            data_bars_ask=self.data_bars_ask,
            clock=self.test_clock,
            logger=Logger())

        self.client.create_data_providers()

    def test_can_initialize_client_with_data(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual(all(self.bid_data_1min), all(self.client.data_bars_bid[self.usdjpy.symbol][Resolution.MINUTE]))
        self.assertEqual(all(self.ask_data_1min), all(self.client.data_bars_bid[self.usdjpy.symbol][Resolution.MINUTE]))
        self.assertEqual(all(self.bid_data_1min.index), all(self.client.data_minute_index))

    def test_can_set_initial_iteration(self):
        # Arrange
        start = datetime(2013, 1, 2, 0, 0, 0, 0, tzinfo=timezone.utc)
        dummy = []

        # Act
        self.client.subscribe_bars(TestStubs.bartype_usdjpy_1min_bid(), dummy.append)
        self.client.set_initial_iteration(start, timedelta(minutes=1))

        # Assert
        self.assertEqual(1440, self.client.iteration)
        self.assertEqual(start, self.client.time_now())
        self.assertEqual(1440, self.client.data_providers[self.usdjpy.symbol].iterations[TestStubs.bartype_usdjpy_1min_bid()])
        self.assertEqual(start, self.client.data_providers[self.usdjpy.symbol].bars[TestStubs.bartype_usdjpy_1min_bid()][1440].timestamp)

    def test_can_iterate_bar_data(self):
        # Arrange
        receiver = ObjectStorer()
        self.client.subscribe_bars(TestStubs.bartype_usdjpy_1min_bid(), receiver.store_2)

        start_datetime = datetime(2013, 1, 1, 0, 0, 0, 0, tzinfo=timezone.utc)

        # Act
        for x in range(1000):
            self.test_clock.set_time(start_datetime + timedelta(minutes=x))
            self.client.iterate()

        # Assert
        self.assertEqual(1000, len(receiver.get_store()))
        self.assertTrue(self.client.data_minute_index[0] == self.client.data_providers[self.usdjpy.symbol].bars[TestStubs.bartype_usdjpy_1min_bid()][0].timestamp)
