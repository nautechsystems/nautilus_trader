#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_backtest_engine.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import pandas as pd
import unittest

from datetime import datetime, timezone

from inv_trader.model.enums import Resolution
from inv_trader.backtest.config import BacktestConfig
from inv_trader.backtest.models import FillModel
from inv_trader.backtest.engine import BacktestEngine
from test_kit.strategies import EmptyStrategy, EMACross
from test_kit.data import TestDataProvider
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
USDJPY_FXCM = TestStubs.instrument_usdjpy().symbol


class BacktestEngineTests(unittest.TestCase):

    def setUp(self):
        bid_data_1min = TestDataProvider.usdjpy_1min_bid()
        ask_data_1min = TestDataProvider.usdjpy_1min_ask()

        instruments = [TestStubs.instrument_usdjpy()]
        tick_data = {USDJPY_FXCM: pd.DataFrame()}
        bid_data = {USDJPY_FXCM: {Resolution.MINUTE: bid_data_1min}}
        ask_data = {USDJPY_FXCM: {Resolution.MINUTE: ask_data_1min}}

        strategies = [EmptyStrategy()]
        config = BacktestConfig()

        self.engine = BacktestEngine(
            instruments=instruments,
            data_ticks=tick_data,
            data_bars_bid=bid_data,
            data_bars_ask=ask_data,
            strategies=strategies,
            fill_model=FillModel(),
            config=config)

    def tearDown(self):
        self.engine.dispose()

    def test_initialization(self):
        self.assertEqual(TestStubs.instrument_usdjpy(), self.engine.instruments[0])
        self.assertEqual(1, len(self.engine.trader.strategy_status()))

    def test_can_run_empty_strategy(self):
        # Arrange
        start = datetime(2013, 1, 1, 0, 0, 0, 0, tzinfo=timezone.utc)
        stop = datetime(2013, 2, 1, 0, 0, 0, 0, tzinfo=timezone.utc)

        # Act
        self.engine.run(start, stop)

        # Assert
        self.assertEqual(44641, self.engine.iteration)

    def test_can_reset_engine_(self):
        # Arrange
        start = datetime(2013, 1, 1, 0, 0, 0, 0, tzinfo=timezone.utc)
        stop = datetime(2013, 2, 1, 0, 0, 0, 0, tzinfo=timezone.utc)

        self.engine.run(start, stop)

        # Act
        self.engine.reset()

        # Assert
        self.assertEqual(0, self.engine.iteration)  # No exceptions raised

    def test_can_run_ema_cross_strategy(self):
        # Arrange
        instrument = TestStubs.instrument_usdjpy()
        bar_type = TestStubs.bartype_usdjpy_1min_bid()

        strategies = [EMACross(order_id_tag='001',
                               instrument=instrument,
                               bar_type=bar_type,
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
        self.assertEqual(2881, self.engine.data_client.data_providers[USDJPY_FXCM].iterations[TestStubs.bartype_usdjpy_1min_bid()])
        self.assertEqual(1441, strategies[0].fast_ema.count)
        self.assertEqual(-12068.51953125, self.engine.get_performance_stats()['PNL'])  # Money represented as float here

    def test_can_reset_and_rerun_ema_cross_strategy_returns_identical_performance(self):
        # Arrange
        instrument = TestStubs.instrument_usdjpy()
        bar_type = TestStubs.bartype_usdjpy_1min_bid()

        strategies = [EMACross(order_id_tag='001',
                               instrument=instrument,
                               bar_type=bar_type,
                               risk_bp=10,
                               fast_ema=10,
                               slow_ema=20,
                               atr_period=20,
                               sl_atr_multiple=2.0)]

        self.engine.change_strategies(strategies)

        start = datetime(2013, 1, 2, 0, 0, 0, 0, tzinfo=timezone.utc)
        stop = datetime(2013, 1, 3, 0, 0, 0, 0, tzinfo=timezone.utc)

        self.engine.run(start, stop)

        # Act
        result1 = self.engine.portfolio.analyzer.get_returns()

        self.engine.reset()
        self.engine.run(start, stop)

        result2 = self.engine.portfolio.analyzer.get_returns()

        # Assert
        self.assertEqual(all(result1), all(result2))

    def test_can_run_multiple_strategies(self):
        # Arrange
        instrument = TestStubs.instrument_usdjpy()
        bar_type = TestStubs.bartype_usdjpy_1min_bid()

        strategies = [EMACross(order_id_tag='001',
                               instrument=instrument,
                               bar_type=bar_type,
                               risk_bp=10,
                               fast_ema=10,
                               slow_ema=20,
                               atr_period=20,
                               sl_atr_multiple=2.0),
                      EMACross(order_id_tag='002',
                               instrument=instrument,
                               bar_type=bar_type,
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
        self.assertEqual(2881, self.engine.data_client.data_providers[USDJPY_FXCM].iterations[TestStubs.bartype_usdjpy_1min_bid()])
        self.assertEqual(1441, strategies[0].fast_ema.count)
