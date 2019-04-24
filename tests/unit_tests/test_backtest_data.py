#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_backtest_data.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from datetime import datetime, timezone, timedelta
from pandas import Timestamp

from inv_trader.common.clock import TestClock
from inv_trader.common.logger import TestLogger
from inv_trader.model.enums import Resolution
from inv_trader.backtest.data import BacktestDataClient
from test_kit.objects import ObjectStorer
from test_kit.data import TestDataProvider
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
USDJPY_FXCM = TestStubs.instrument_usdjpy().symbol


class BacktestDataClientTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.usdjpy = TestStubs.instrument_usdjpy()
        self.bid_data_1min = TestDataProvider.usdjpy_1min_bid().iloc[:2000]
        self.ask_data_1min = TestDataProvider.usdjpy_1min_ask().iloc[:2000]

        self.test_clock = TestClock()
        self.client = BacktestDataClient(
            instruments=[TestStubs.instrument_usdjpy()],
            data_ticks={USDJPY_FXCM: TestDataProvider.usdjpy_test_ticks()},
            data_bars_bid={USDJPY_FXCM: {Resolution.MINUTE: self.bid_data_1min}},
            data_bars_ask={USDJPY_FXCM: {Resolution.MINUTE: self.ask_data_1min}},
            clock=self.test_clock,
            logger=TestLogger())

    def test_can_initialize_client_with_data(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual(all(self.bid_data_1min), all(self.client.data_bars_bid[USDJPY_FXCM][Resolution.MINUTE]))
        self.assertEqual(all(self.ask_data_1min), all(self.client.data_bars_bid[USDJPY_FXCM][Resolution.MINUTE]))
        self.assertEqual(all(self.bid_data_1min.index), all(self.client.data_minute_index))

    def test_can_set_initial_iteration(self):
        # Arrange
        start = datetime(2013, 1, 2, 0, 0, 0, 0, tzinfo=timezone.utc)
        dummy = []

        # Act
        self.client.subscribe_ticks(USDJPY_FXCM, dummy.append)
        self.client.subscribe_bars(TestStubs.bartype_usdjpy_1min_bid(), dummy.append)
        self.client.set_initial_iteration(start, timedelta(minutes=1))

        # Assert
        self.assertEqual(start, self.client.time_now())
        self.assertTrue(self.client.data_providers[USDJPY_FXCM].has_ticks)
        self.assertEqual(999, self.client.data_providers[USDJPY_FXCM].tick_index)
        self.assertEqual(1440, self.client.data_providers[USDJPY_FXCM].iterations[TestStubs.bartype_usdjpy_1min_bid()])
        self.assertEqual(start, self.client.data_providers[USDJPY_FXCM].bars[TestStubs.bartype_usdjpy_1min_bid()][1440].timestamp)

    def test_can_iterate_all_ticks(self):
        # Arrange
        receiver = ObjectStorer()
        self.client.subscribe_ticks(self.usdjpy.symbol, receiver.store)

        start_datetime = datetime(2013, 1, 1, 22, 0, 0, 0, tzinfo=timezone.utc)

        # Act
        for x in range(1000):
            self.test_clock.set_time(start_datetime + timedelta(minutes=x))
            ticks_list = self.client.iterate_ticks(self.test_clock.time_now())
            for tick in ticks_list:
                self.client.process_tick(tick)

        # Assert
        self.assertEqual(len(self.client.data_providers[USDJPY_FXCM].ticks), len(receiver.get_store()))
        self.assertTrue(self.client.data_providers[USDJPY_FXCM].has_ticks)

    def test_can_iterate_some_ticks(self):
        # Arrange
        receiver = ObjectStorer()
        self.client.subscribe_ticks(USDJPY_FXCM, receiver.store)

        start_datetime = datetime(2013, 1, 1, 22, 0, 0, 0, tzinfo=timezone.utc)

        # Act
        for x in range(30):
            self.test_clock.set_time(start_datetime + timedelta(minutes=x))
            ticks_list = self.client.iterate_ticks(self.test_clock.time_now())
            for tick in ticks_list:
                self.client.process_tick(tick)

        # Assert
        self.assertTrue(self.client.data_providers[USDJPY_FXCM].has_ticks)
        self.assertEqual(655, len(receiver.get_store()))
        self.assertEqual(Timestamp('2013-01-01 22:28:53.319000+0000', tz='UTC'), receiver.get_store().pop().timestamp)

    def test_can_iterate_bars(self):
        # Arrange
        receiver = ObjectStorer()
        self.client.subscribe_bars(TestStubs.bartype_usdjpy_1min_bid(), receiver.store_2)

        start_datetime = datetime(2013, 1, 1, 0, 0, 0, 0, tzinfo=timezone.utc)

        # Act
        for x in range(1000):
            self.test_clock.set_time(start_datetime + timedelta(minutes=x))
            bars = self.client.iterate_bars(self.test_clock.time_now())
            self.client.process_bars(bars)

        # Assert
        self.assertEqual(1000, len(receiver.get_store()))
        self.assertTrue(self.client.data_minute_index[0] == self.client.data_providers[USDJPY_FXCM].bars[TestStubs.bartype_usdjpy_1min_bid()][0].timestamp)
        self.assertEqual(Timestamp('2013-01-01 16:39:00+0000', tz='UTC'), receiver.get_store()[999][1].timestamp)

    def test_can_iterate_ticks_and_bars(self):
        # Arrange
        receiver = ObjectStorer()
        self.client.subscribe_ticks(USDJPY_FXCM, receiver.store)
        self.client.subscribe_bars(TestStubs.bartype_usdjpy_1min_bid(), receiver.store_2)
        self.client.subscribe_bars(TestStubs.bartype_usdjpy_1min_ask(), receiver.store_2)

        start_datetime = datetime(2013, 1, 1, 22, 0, 0, 0, tzinfo=timezone.utc)

        self.client.set_initial_iteration(start_datetime, timedelta(minutes=1))

        # Act
        for x in range(30):
            self.test_clock.set_time(start_datetime + timedelta(minutes=x))
            ticks_list = self.client.iterate_ticks(self.test_clock.time_now())
            for tick in ticks_list:
                self.client.process_tick(tick)
            self.client.get_next_execution_bars(self.test_clock.time_now())  # Testing this does not cause iteration errors
            bars = self.client.iterate_bars(self.test_clock.time_now())
            self.client.process_bars(bars)

        print(receiver.get_store())
        # Assert
        self.assertTrue(self.client.data_providers[USDJPY_FXCM].has_ticks)
        self.assertEqual(715, len(receiver.get_store()))
        self.assertEqual(Timestamp('2013-01-01 22:29:00+0000', tz='UTC'), receiver.get_store().pop()[1].timestamp)
