# -------------------------------------------------------------------------------------------------
# <copyright file="test_backtest_data.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import pandas as pd

from datetime import datetime, timezone, timedelta
from pandas import Timestamp

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logger import TestLogger
from nautilus_trader.model.enums import Resolution
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.backtest.data import BacktestDataClient

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

    def test_can_initialize_client_with_data(self):
        # Arrange
        client = BacktestDataClient(
            venue=Venue('FXCM'),
            instruments=[TestStubs.instrument_usdjpy()],
            data_ticks={USDJPY_FXCM: pd.DataFrame()},
            data_bars_bid={USDJPY_FXCM: {Resolution.MINUTE: self.bid_data_1min}},
            data_bars_ask={USDJPY_FXCM: {Resolution.MINUTE: self.ask_data_1min}},
            clock=self.test_clock,
            logger=TestLogger())

        # Act
        # Assert
        self.assertEqual(all(self.bid_data_1min), all(client.data_bars_bid[USDJPY_FXCM][Resolution.MINUTE]))
        self.assertEqual(all(self.ask_data_1min), all(client.data_bars_bid[USDJPY_FXCM][Resolution.MINUTE]))
        self.assertEqual(pd.to_datetime(self.bid_data_1min.index[0], utc=True), client.execution_data_index_min)

    def test_can_set_initial_iteration(self):
        # Arrange
        client = BacktestDataClient(
            venue=Venue('FXCM'),
            instruments=[TestStubs.instrument_usdjpy()],
            data_ticks={USDJPY_FXCM: TestDataProvider.usdjpy_test_ticks()},
            data_bars_bid={USDJPY_FXCM: {Resolution.MINUTE: self.bid_data_1min}},
            data_bars_ask={USDJPY_FXCM: {Resolution.MINUTE: self.ask_data_1min}},
            clock=self.test_clock,
            logger=TestLogger())

        start = datetime(2013, 1, 2, 0, 0, 0, 0, tzinfo=timezone.utc)
        dummy = []

        # Act
        client.subscribe_ticks(USDJPY_FXCM, dummy.append)
        client.subscribe_bars(TestStubs.bartype_usdjpy_1min_bid(), dummy.append)
        client.set_initial_iteration_indexes(start)

        # Assert
        self.assertEqual(start, client.time_now())
        self.assertEqual(999, client.data_providers[USDJPY_FXCM].tick_index)
        self.assertEqual(1440, client.data_providers[USDJPY_FXCM].iterations[TestStubs.bartype_usdjpy_1min_bid()])
        self.assertEqual(Timestamp('2013-01-02 00:01:00+0000', tz='UTC'), client.data_providers[USDJPY_FXCM].bars[TestStubs.bartype_usdjpy_1min_bid()][1441].timestamp)

    def test_can_iterate_all_ticks(self):
        # Arrange
        client = BacktestDataClient(
            venue=Venue('FXCM'),
            instruments=[TestStubs.instrument_usdjpy()],
            data_ticks={USDJPY_FXCM: TestDataProvider.usdjpy_test_ticks()},
            data_bars_bid={USDJPY_FXCM: {Resolution.MINUTE: self.bid_data_1min}},
            data_bars_ask={USDJPY_FXCM: {Resolution.MINUTE: self.ask_data_1min}},
            clock=self.test_clock,
            logger=TestLogger())

        receiver = ObjectStorer()
        client.subscribe_ticks(self.usdjpy.symbol, receiver.store)

        start_datetime = datetime(2013, 1, 1, 22, 0, 0, 0, tzinfo=timezone.utc)

        # Act
        for x in range(1000):
            self.test_clock.set_time(start_datetime + timedelta(minutes=x))
            ticks_list = client.iterate_ticks(self.test_clock.time_now())
            for tick in ticks_list:
                client.process_tick(tick)

        # Assert
        self.assertEqual(len(client.data_providers[USDJPY_FXCM].ticks), len(receiver.get_store()))

    def test_can_iterate_some_ticks(self):
        # Arrange
        client = BacktestDataClient(
            venue=Venue('FXCM'),
            instruments=[TestStubs.instrument_usdjpy()],
            data_ticks={USDJPY_FXCM: TestDataProvider.usdjpy_test_ticks()},
            data_bars_bid={USDJPY_FXCM: {Resolution.MINUTE: self.bid_data_1min}},
            data_bars_ask={USDJPY_FXCM: {Resolution.MINUTE: self.ask_data_1min}},
            clock=self.test_clock,
            logger=TestLogger())

        receiver = ObjectStorer()
        client.subscribe_ticks(USDJPY_FXCM, receiver.store)

        start_datetime = datetime(2013, 1, 1, 22, 0, 0, 0, tzinfo=timezone.utc)

        # Act
        for x in range(30):
            self.test_clock.set_time(start_datetime + timedelta(minutes=x))
            ticks_list = client.iterate_ticks(self.test_clock.time_now())
            for tick in ticks_list:
                client.process_tick(tick)

        # Assert
        self.assertEqual(655, len(receiver.get_store()))
        self.assertEqual(Timestamp('2013-01-01 22:28:53.319000+0000', tz='UTC'), receiver.get_store().pop().timestamp)

    def test_can_iterate_bars(self):
        # Arrange
        client = BacktestDataClient(
            venue=Venue('FXCM'),
            instruments=[TestStubs.instrument_usdjpy()],
            data_ticks={USDJPY_FXCM: pd.DataFrame()},
            data_bars_bid={USDJPY_FXCM: {Resolution.MINUTE: self.bid_data_1min}},
            data_bars_ask={USDJPY_FXCM: {Resolution.MINUTE: self.ask_data_1min}},
            clock=self.test_clock,
            logger=TestLogger())

        receiver = ObjectStorer()
        client.subscribe_bars(TestStubs.bartype_usdjpy_1min_bid(), receiver.store_2)

        start_datetime = datetime(2013, 1, 1, 0, 0, 0, 0, tzinfo=timezone.utc)

        # Act
        for x in range(1000):
            self.test_clock.set_time(start_datetime + timedelta(minutes=x))
            bars = client.iterate_bars(self.test_clock.time_now())
            client.process_bars(bars)

        # Assert
        self.assertEqual(1000, len(receiver.get_store()))
        self.assertTrue(client.execution_data_index_min == client.data_providers[USDJPY_FXCM].bars[TestStubs.bartype_usdjpy_1min_bid()][0].timestamp)
        self.assertEqual(Timestamp('2013-01-01 16:39:00+0000', tz='UTC'), receiver.get_store()[999][1].timestamp)

    def test_can_iterate_ticks_and_bars(self):
        # Arrange
        client = BacktestDataClient(
            venue=Venue('FXCM'),
            instruments=[TestStubs.instrument_usdjpy()],
            data_ticks={USDJPY_FXCM: pd.DataFrame()},
            data_bars_bid={USDJPY_FXCM: {Resolution.MINUTE: self.bid_data_1min}},
            data_bars_ask={USDJPY_FXCM: {Resolution.MINUTE: self.ask_data_1min}},
            clock=self.test_clock,
            logger=TestLogger())

        receiver = ObjectStorer()
        client.subscribe_ticks(USDJPY_FXCM, receiver.store)
        client.subscribe_bars(TestStubs.bartype_usdjpy_1min_bid(), receiver.store_2)
        client.subscribe_bars(TestStubs.bartype_usdjpy_1min_ask(), receiver.store_2)

        start_datetime = datetime(2013, 1, 1, 22, 0, 0, 0, tzinfo=timezone.utc)

        client.set_initial_iteration_indexes(start_datetime)

        # Act
        for x in range(30):
            self.test_clock.set_time(start_datetime + timedelta(minutes=x))
            ticks_list = client.iterate_ticks(self.test_clock.time_now())
            for tick in ticks_list:
                client.process_tick(tick)
            client.get_next_execution_bars(self.test_clock.time_now())  # Testing this does not cause iteration errors
            bars = client.iterate_bars(self.test_clock.time_now())
            client.process_bars(bars)

        # print(receiver.get_store())
        # Assert
        self.assertEqual(90, len(receiver.get_store()))
        self.assertEqual(Timestamp('2013-01-01 22:29:00+0000', tz='UTC'), receiver.get_store().pop()[1].timestamp)
