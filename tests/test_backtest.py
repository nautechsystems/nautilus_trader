#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_backtest.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import pandas as pd
import unittest

from datetime import datetime, timezone, timedelta

from inv_trader.model.enums import Resolution
from inv_trader.model.objects import BarType
from inv_trader.backtest.data import BacktestDataClient
from inv_trader.backtest.engine import BacktestEngine
from test_kit.strategies import TestStrategy1, EMACross
from test_kit.data import TestDataProvider
from test_kit.stubs import TestStubs


class BacktestDataClientTests(unittest.TestCase):

    def test_can_initialize_client_with_data(self):
        # Arrange
        usdjpy = TestStubs.instrument_usdjpy()
        bid_data_1min = TestDataProvider.usdjpy_1min_bid()
        ask_data_1min = TestDataProvider.usdjpy_1min_ask()

        instruments = [TestStubs.instrument_usdjpy()]
        tick_data = {usdjpy.symbol: pd.DataFrame()}
        bid_data = {usdjpy.symbol: {Resolution.MINUTE: bid_data_1min}}
        ask_data = {usdjpy.symbol: {Resolution.MINUTE: ask_data_1min}}

        # Act
        client = BacktestDataClient(instruments=instruments,
                                    tick_data=tick_data,
                                    bar_data_bid=bid_data,
                                    bar_data_ask=ask_data)

        # Assert
        self.assertEqual(all(bid_data_1min), all(client.bar_data_bid[usdjpy.symbol][Resolution.MINUTE]))
        self.assertEqual(all(ask_data_1min), all(client.bar_data_bid[usdjpy.symbol][Resolution.MINUTE]))

    def test_can_iterate_bar_data(self):
        # Arrange
        usdjpy = TestStubs.instrument_usdjpy()
        bid_data_1min = TestDataProvider.usdjpy_1min_bid()
        ask_data_1min = TestDataProvider.usdjpy_1min_ask()

        instruments = [TestStubs.instrument_usdjpy()]
        tick_data = {usdjpy.symbol: pd.DataFrame()}
        bid_data = {usdjpy.symbol: {Resolution.MINUTE: bid_data_1min}}
        ask_data = {usdjpy.symbol: {Resolution.MINUTE: ask_data_1min}}

        client = BacktestDataClient(instruments=instruments,
                                    tick_data=tick_data,
                                    bar_data_bid=bid_data,
                                    bar_data_ask=ask_data)

        receiver = []
        client.subscribe_bars(TestStubs.bartype_usdjpy_1min_bid(), receiver.append)

        start_datetime = datetime(2013, 1, 1, 0, 0, 0, 0)

        # Act
        for x in range(1000):
            client.iterate(start_datetime + timedelta(minutes=x))

        # Assert
        self.assertEqual(1000, len(receiver))


class BacktestEngineTests(unittest.TestCase):

    def test_can_initialize_engine_with_data(self):
        # Arrange
        usdjpy = TestStubs.instrument_usdjpy()
        bid_data_1min = TestDataProvider.usdjpy_1min_bid()
        ask_data_1min = TestDataProvider.usdjpy_1min_ask()

        instruments = [TestStubs.instrument_usdjpy()]
        tick_data = {usdjpy.symbol: pd.DataFrame()}
        bid_data = {usdjpy.symbol: {Resolution.MINUTE: bid_data_1min}}
        ask_data = {usdjpy.symbol: {Resolution.MINUTE: ask_data_1min}}

        strategies = [TestStrategy1(TestStubs.bartype_usdjpy_1min_bid())]

        # Act
        engine = BacktestEngine(instruments=instruments,
                                tick_data=tick_data,
                                bar_data_bid=bid_data,
                                bar_data_ask=ask_data,
                                strategies=strategies)

        # Assert
        self.assertEqual(all(bid_data), all(engine.data_client.bar_data_bid))
        self.assertEqual(all(ask_data), all(engine.data_client.bar_data_bid))

    def test_can_run(self):
        # Arrange
        usdjpy = TestStubs.instrument_usdjpy()
        bid_data_1min = TestDataProvider.usdjpy_1min_bid()
        ask_data_1min = TestDataProvider.usdjpy_1min_ask()

        instruments = [TestStubs.instrument_usdjpy()]
        tick_data = {usdjpy.symbol: pd.DataFrame()}
        bid_data = {usdjpy.symbol: {Resolution.MINUTE: bid_data_1min}}
        ask_data = {usdjpy.symbol: {Resolution.MINUTE: ask_data_1min}}

        strategies = [EMACross(label='EMACross_Test',
                               order_id_tag='01',
                               instrument=usdjpy,
                               bar_type=TestStubs.bartype_usdjpy_1min_bid(),
                               position_size=100000,
                               fast_ema=10,
                               slow_ema=20,
                               atr_period=20,
                               sl_atr_multiple=2.0)]
        engine = BacktestEngine(instruments=instruments,
                                tick_data=tick_data,
                                bar_data_bid=bid_data,
                                bar_data_ask=ask_data,
                                strategies=strategies)

        start = datetime(2013, 1, 1, 0, 0, 0, 0, tzinfo=timezone.utc)
        stop = datetime(2013, 1, 2, 0, 0, 0, 0, tzinfo=timezone.utc)

        # Act
        # engine.run(start, stop)



