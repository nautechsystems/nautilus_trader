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

from inv_trader.core.decimal import Decimal
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


class BacktestEngineTests(unittest.TestCase):

    def test_can_run_empty_strategy(self):
        # Arrange
        usdjpy = TestStubs.instrument_usdjpy()
        bid_data_1min = TestDataProvider.usdjpy_1min_bid()
        ask_data_1min = TestDataProvider.usdjpy_1min_ask()

        instruments = [TestStubs.instrument_usdjpy()]
        tick_data = {usdjpy.symbol: pd.DataFrame()}
        bid_data = {usdjpy.symbol: {Resolution.MINUTE: bid_data_1min}}
        ask_data = {usdjpy.symbol: {Resolution.MINUTE: ask_data_1min}}

        strategies = [EmptyStrategy()]

        config = BacktestConfig(console_prints=True)
        engine = BacktestEngine(instruments=instruments,
                                data_ticks=tick_data,
                                data_bars_bid=bid_data,
                                data_bars_ask=ask_data,
                                strategies=strategies,
                                config=config)

        start = datetime(2013, 1, 1, 0, 0, 0, 0, tzinfo=timezone.utc)
        stop = datetime(2013, 2, 1, 0, 0, 0, 0, tzinfo=timezone.utc)

        # Act
        engine.run(start, stop)

        # Assert
        self.assertEqual(44640, engine.data_client.iteration)
        self.assertEqual(44640, engine.exec_client.iteration)

    def test_can_run(self):
        # Arrange
        usdjpy = TestStubs.instrument_usdjpy()
        bid_data_1min = TestDataProvider.usdjpy_1min_bid()
        ask_data_1min = TestDataProvider.usdjpy_1min_ask()

        instruments = [TestStubs.instrument_usdjpy()]
        tick_data = {usdjpy.symbol: pd.DataFrame()}
        bid_data = {usdjpy.symbol: {Resolution.MINUTE: bid_data_1min}}
        ask_data = {usdjpy.symbol: {Resolution.MINUTE: ask_data_1min}}

        strategies = [EMACross(label='001',
                               order_id_tag='01',
                               instrument=usdjpy,
                               bar_type=TestStubs.bartype_usdjpy_1min_bid(),
                               position_size=100000,
                               fast_ema=10,
                               slow_ema=20,
                               atr_period=20,
                               sl_atr_multiple=2.0)]

        config = BacktestConfig(slippage_ticks=1,
                                bypass_logging=False,
                                console_prints=True)
        engine = BacktestEngine(instruments=instruments,
                                data_ticks=tick_data,
                                data_bars_bid=bid_data,
                                data_bars_ask=ask_data,
                                strategies=strategies,
                                config=config)

        start = datetime(2013, 1, 2, 0, 0, 0, 0, tzinfo=timezone.utc)
        stop = datetime(2013, 1, 3, 0, 0, 0, 0, tzinfo=timezone.utc)

        # Act
        engine.run(start, stop)

        # Assert
        self.assertEqual(2880, engine.data_client.data_providers[usdjpy.symbol].iterations[TestStubs.bartype_usdjpy_1min_bid()])
        self.assertEqual(1440, strategies[0].fast_ema.count)
