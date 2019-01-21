#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_backtest.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import cProfile
import pstats
import pandas as pd
import unittest

from datetime import datetime, timezone, timedelta

from inv_trader.core.decimal import Decimal
from inv_trader.common.clock import TestClock
from inv_trader.common.logger import Logger
from inv_trader.model.enums import Resolution
from inv_trader.model.enums import Venue, OrderSide, OrderStatus, TimeInForce
from inv_trader.model.identifiers import Label, OrderId, PositionId
from inv_trader.model.objects import Symbol
from inv_trader.strategy import TradeStrategy
from inv_trader.backtest.data import BacktestDataClient
from inv_trader.backtest.execution import BacktestExecClient
from inv_trader.backtest.engine import BacktestConfig, BacktestEngine
from test_kit.objects import ObjectStorer
from test_kit.strategies import TestStrategy1, EMACross
from test_kit.data import TestDataProvider
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
USDJPY_FXCM = Symbol('USDJPY', Venue.FXCM)


class BacktestEngineTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        usdjpy = TestStubs.instrument_usdjpy()
        bid_data_1min = TestDataProvider.usdjpy_1min_bid()
        ask_data_1min = TestDataProvider.usdjpy_1min_ask()

        self.instruments = [TestStubs.instrument_usdjpy()]
        self.tick_data = {usdjpy.symbol: pd.DataFrame()}
        self.bid_data = {usdjpy.symbol: {Resolution.MINUTE: bid_data_1min}}
        self.ask_data = {usdjpy.symbol: {Resolution.MINUTE: ask_data_1min}}

        self.strategies = [TestStrategy1(TestStubs.bartype_usdjpy_1min_bid())]

        self.engine = BacktestEngine(instruments=self.instruments,
                                     tick_data=self.tick_data,
                                     bar_data_bid=self.bid_data,
                                     bar_data_ask=self.ask_data,
                                     strategies=self.strategies)

    def test_can_initialize_engine_with_data(self):
        # Arrange
        # Act
        # Assert
        # Does not throw exception
        self.assertEqual(all(self.bid_data), all(self.engine.data_client.bar_data_bid))
        self.assertEqual(all(self.ask_data), all(self.engine.data_client.bar_data_bid))

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

        config = BacktestConfig(console_prints=True)
        engine = BacktestEngine(instruments=instruments,
                                tick_data=tick_data,
                                bar_data_bid=bid_data,
                                bar_data_ask=ask_data,
                                strategies=strategies,
                                config=config)

        start = datetime(2013, 1, 1, 22, 0, 0, 0, tzinfo=timezone.utc)
        stop = datetime(2013, 1, 10, 0, 0, 0, 0, tzinfo=timezone.utc)

        cProfile.runctx('engine.run(start, stop)', globals(), locals(), 'Profile.prof')
        s = pstats.Stats("Profile.prof")
        s.strip_dirs().sort_stats("time").print_stats()
