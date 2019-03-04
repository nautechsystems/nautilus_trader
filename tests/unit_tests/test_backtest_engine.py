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
        self.bid_data_1min = TestDataProvider.usdjpy_1min_bid()
        self.ask_data_1min = TestDataProvider.usdjpy_1min_ask()

        self.instruments = [TestStubs.instrument_usdjpy()]
        self.tick_data = {self.usdjpy.symbol: pd.DataFrame()}
        self.bid_data = {self.usdjpy.symbol: {Resolution.MINUTE: self.bid_data_1min}}
        self.ask_data = {self.usdjpy.symbol: {Resolution.MINUTE: self.ask_data_1min}}

    def test_initialization(self):
        strategies = [EmptyStrategy()]

        engine = BacktestEngine(instruments=self.instruments,
                                data_ticks=self.tick_data,
                                data_bars_bid=self.bid_data,
                                data_bars_ask=self.ask_data,
                                strategies=strategies)

        self.assertEqual(self.usdjpy, engine.instruments[0])
        self.assertEqual(strategies[0], engine.trader.strategies[0])

    def test_can_run_empty_strategy(self):
        # Arrange
        strategies = [EmptyStrategy()]

        engine = BacktestEngine(instruments=self.instruments,
                                data_ticks=self.tick_data,
                                data_bars_bid=self.bid_data,
                                data_bars_ask=self.ask_data,
                                strategies=strategies)

        start = datetime(2013, 1, 1, 0, 0, 0, 0, tzinfo=timezone.utc)
        stop = datetime(2013, 2, 1, 0, 0, 0, 0, tzinfo=timezone.utc)

        # Act
        engine.run(start, stop)

        # Assert
        self.assertEqual(44641, engine.data_client.iteration)
        self.assertEqual(44641, engine.exec_client.iteration)

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

        config = BacktestConfig(leverage=50,
                                slippage_ticks=1)
        engine = BacktestEngine(instruments=self.instruments,
                                data_ticks=self.tick_data,
                                data_bars_bid=self.bid_data,
                                data_bars_ask=self.ask_data,
                                strategies=strategies,
                                config=config)

        start = datetime(2013, 1, 2, 0, 0, 0, 0, tzinfo=timezone.utc)
        stop = datetime(2013, 1, 3, 0, 0, 0, 0, tzinfo=timezone.utc)

        # Act
        engine.run(start, stop)

        # Assert
        self.assertEqual(2881, engine.data_client.data_providers[self.usdjpy.symbol].iterations[TestStubs.bartype_usdjpy_1min_bid()])
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

        config = BacktestConfig(leverage=50,
                                slippage_ticks=1)
        engine = BacktestEngine(instruments=self.instruments,
                                data_ticks=self.tick_data,
                                data_bars_bid=self.bid_data,
                                data_bars_ask=self.ask_data,
                                strategies=strategies,
                                config=config)

        start = datetime(2013, 1, 2, 0, 0, 0, 0, tzinfo=timezone.utc)
        stop = datetime(2013, 1, 3, 0, 0, 0, 0, tzinfo=timezone.utc)

        # Act
        engine.run(start, stop)

        # Assert
        self.assertEqual(2881, engine.data_client.data_providers[self.usdjpy.symbol].iterations[TestStubs.bartype_usdjpy_1min_bid()])
        self.assertEqual(1441, strategies[0].fast_ema.count)
