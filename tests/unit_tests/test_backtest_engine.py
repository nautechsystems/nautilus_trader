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

from datetime import datetime, timezone

from inv_trader.model.enums import Resolution
from inv_trader.model.enums import Venue
from inv_trader.model.objects import Symbol
from inv_trader.backtest.engine import BacktestConfig, BacktestEngine
from test_kit.strategies import EmptyStrategy, EMACross
from test_kit.data import TestDataProvider
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
USDJPY_FXCM = Symbol('USDJPY', Venue.FXCM)


class BacktestEngineTests(unittest.TestCase):

    def setUp(self):
        self.usdjpy = TestStubs.instrument_usdjpy()
        bid_data_1min = TestDataProvider.usdjpy_1min_bid()
        ask_data_1min = TestDataProvider.usdjpy_1min_ask()

        instruments = [TestStubs.instrument_usdjpy()]
        tick_data = {self.usdjpy.symbol: pd.DataFrame()}
        bid_data = {self.usdjpy.symbol: {Resolution.MINUTE: bid_data_1min}}
        ask_data = {self.usdjpy.symbol: {Resolution.MINUTE: ask_data_1min}}

        strategies = [EmptyStrategy()]
        config = BacktestConfig(
            leverage=50,
            slippage_ticks=1)

        self.engine = BacktestEngine(
            instruments=instruments,
            data_ticks=tick_data,
            data_bars_bid=bid_data,
            data_bars_ask=ask_data,
            strategies=strategies,
            config=config)

    def tearDown(self):
        self.engine.dispose()

    def test_initialization(self):
        self.assertEqual(self.usdjpy, self.engine.instruments[0])
        self.assertEqual(1, self.engine.trader.strategy_count())

    def test_can_run_empty_strategy(self):
        # Arrange
        start = datetime(2013, 1, 1, 0, 0, 0, 0, tzinfo=timezone.utc)
        stop = datetime(2013, 2, 1, 0, 0, 0, 0, tzinfo=timezone.utc)

        # Act
        self.engine.run(start, stop)

        # Assert
        self.assertEqual(44641, self.engine.data_client.iteration)
        self.assertEqual(44641, self.engine.exec_client.iteration)

    def test_can_run_ema_cross_strategy(self):
        # Arrange
        strategies = [EMACross(label='001',
                               id_tag_trader='001',
                               id_tag_strategy='001',
                               instrument=self.usdjpy,
                               bar_type=TestStubs.bartype_usdjpy_1min_bid(),
                               risk_bp=10,
                               fast_ema=10,
                               slow_ema=20,
                               atr_period=20,
                               sl_atr_multiple=2.0)]

        self.engine.change_strategies(strategies)

        start = datetime(2013, 1, 2, 0, 0, 0, 0, tzinfo=timezone.utc)
        stop = datetime(2013, 1, 3, 0, 0, 0, 0, tzinfo=timezone.utc)

        # Act
        self.engine.run(start, stop)

        # Assert
        self.assertEqual(2881, self.engine.data_client.data_providers[self.usdjpy.symbol].iterations[TestStubs.bartype_usdjpy_1min_bid()])
        self.assertEqual(1441, strategies[0].fast_ema.count)

    def test_can_run_multiple_strategies(self):
        # Arrange
        strategies = [EMACross(label='001',
                               id_tag_trader='001',
                               id_tag_strategy='001',
                               instrument=self.usdjpy,
                               bar_type=TestStubs.bartype_usdjpy_1min_bid(),
                               risk_bp=10,
                               fast_ema=10,
                               slow_ema=20,
                               atr_period=20,
                               sl_atr_multiple=2.0),
                      EMACross(label='002',
                               id_tag_trader='002',
                               id_tag_strategy='002',
                               instrument=self.usdjpy,
                               bar_type=TestStubs.bartype_usdjpy_1min_bid(),
                               risk_bp=10,
                               fast_ema=10,
                               slow_ema=20,
                               atr_period=20,
                               sl_atr_multiple=2.0)]

        self.engine.change_strategies(strategies)

        start = datetime(2013, 1, 2, 0, 0, 0, 0, tzinfo=timezone.utc)
        stop = datetime(2013, 1, 3, 0, 0, 0, 0, tzinfo=timezone.utc)

        # Act
        self.engine.run(start, stop)

        # Assert
        self.assertEqual(2881, self.engine.data_client.data_providers[self.usdjpy.symbol].iterations[TestStubs.bartype_usdjpy_1min_bid()])
        self.assertEqual(1441, strategies[0].fast_ema.count)
