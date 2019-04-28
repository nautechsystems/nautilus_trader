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

from datetime import datetime
from pandas import Timestamp

from inv_trader.model.enums import Resolution
from inv_trader.model.objects import Tick, Bar
from inv_trader.model.events import TimeEvent
from inv_trader.backtest.config import BacktestConfig
from inv_trader.backtest.models import FillModel
from inv_trader.backtest.engine import BacktestEngine
from test_kit.strategies import EmptyStrategy, EMACross, TickTock
from test_kit.data import TestDataProvider
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
USDJPY_FXCM = TestStubs.instrument_usdjpy().symbol


class BacktestEngineTests(unittest.TestCase):

    def setUp(self):
        instruments = [TestStubs.instrument_usdjpy()]
        tick_data = {USDJPY_FXCM: pd.DataFrame()}
        bid_data = {USDJPY_FXCM: {Resolution.MINUTE: TestDataProvider.usdjpy_1min_bid()}}
        ask_data = {USDJPY_FXCM: {Resolution.MINUTE: TestDataProvider.usdjpy_1min_ask()}}

        self.engine = BacktestEngine(
            instruments=instruments,
            data_ticks=tick_data,
            data_bars_bid=bid_data,
            data_bars_ask=ask_data,
            strategies=[EmptyStrategy('000')],
            fill_model=FillModel(),
            config=BacktestConfig())

    def tearDown(self):
        self.engine.dispose()

    def test_initialization(self):
        self.assertEqual(TestStubs.instrument_usdjpy(), self.engine.instruments[0])
        self.assertEqual(1, len(self.engine.trader.strategy_status()))

    def test_can_run_empty_strategy(self):
        # Arrange
        start = datetime(2013, 1, 1, 0, 0, 0, 0)
        stop = datetime(2013, 2, 1, 0, 0, 0, 0)

        # Act
        self.engine.run(start, stop)

        # Assert
        self.assertEqual(44641, self.engine.iteration)

    def test_can_reset_engine_(self):
        # Arrange
        start = datetime(2013, 1, 1, 0, 0, 0, 0)
        stop = datetime(2013, 2, 1, 0, 0, 0, 0)

        self.engine.run(start, stop)

        # Act
        self.engine.reset()

        # Assert
        self.assertEqual(0, self.engine.iteration)  # No exceptions raised

    def test_can_run_ema_cross_strategy(self):
        # Arrange
        instrument = TestStubs.instrument_usdjpy()
        bar_type = TestStubs.bartype_usdjpy_1min_bid()

        strategies = [EMACross(instrument=instrument,
                               bar_type=bar_type,
                               risk_bp=10,
                               fast_ema=10,
                               slow_ema=20,
                               atr_period=20,
                               sl_atr_multiple=2.0)]

        start = datetime(2013, 1, 2, 0, 0, 0, 0)
        stop = datetime(2013, 1, 3, 0, 0, 0, 0)

        # Act
        self.engine.run(start, stop, strategies=strategies)

        # Assert
        self.assertEqual(2881, self.engine.data_client.data_providers[USDJPY_FXCM].iterations[TestStubs.bartype_usdjpy_1min_bid()])
        self.assertEqual(1441, strategies[0].fast_ema.count)
        self.assertEqual(-12068.51953125, self.engine.get_performance_stats()['PNL'])  # Money represented as float here

    def test_can_reset_and_rerun_ema_cross_strategy_returns_identical_performance(self):
        # Arrange
        instrument = TestStubs.instrument_usdjpy()
        bar_type = TestStubs.bartype_usdjpy_1min_bid()

        strategies = [EMACross(instrument=instrument,
                               bar_type=bar_type,
                               risk_bp=10,
                               fast_ema=10,
                               slow_ema=20,
                               atr_period=20,
                               sl_atr_multiple=2.0)]

        start = datetime(2013, 1, 2, 0, 0, 0, 0)
        stop = datetime(2013, 1, 3, 0, 0, 0, 0)

        self.engine.run(start, stop, strategies=strategies)

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

        strategies = [EMACross(instrument=instrument,
                               bar_type=bar_type,
                               risk_bp=10,
                               fast_ema=10,
                               slow_ema=20,
                               atr_period=20,
                               sl_atr_multiple=2.0,
                               extra_id_tag='001'),
                      EMACross(instrument=instrument,
                               bar_type=bar_type,
                               risk_bp=10,
                               fast_ema=10,
                               slow_ema=20,
                               atr_period=20,
                               sl_atr_multiple=2.0,
                               extra_id_tag='002')]

        start = datetime(2013, 1, 2, 0, 0, 0, 0)
        stop = datetime(2013, 1, 3, 0, 0, 0, 0)

        # Act
        self.engine.run(start, stop, strategies=strategies)

        # Assert
        self.assertEqual(2881, self.engine.data_client.data_providers[USDJPY_FXCM].iterations[TestStubs.bartype_usdjpy_1min_bid()])
        self.assertEqual(1441, strategies[0].fast_ema.count)
        self.assertEqual(1441, strategies[1].fast_ema.count)

    def test_timer_and_alert_sequencing_with_bar_execution(self):
        # Arrange
        instruments = [TestStubs.instrument_usdjpy()]
        tick_data = {USDJPY_FXCM: pd.DataFrame()}
        bid_data = {USDJPY_FXCM: {Resolution.MINUTE: TestDataProvider.usdjpy_1min_bid()}}
        ask_data = {USDJPY_FXCM: {Resolution.MINUTE: TestDataProvider.usdjpy_1min_ask()}}

        instrument = TestStubs.instrument_usdjpy()
        bar_type = TestStubs.bartype_usdjpy_1min_bid()

        tick_tock = TickTock(instrument=instrument, bar_type=bar_type)

        engine = BacktestEngine(
            instruments=instruments,
            data_ticks=tick_data,
            data_bars_bid=bid_data,
            data_bars_ask=ask_data,
            strategies=[tick_tock],
            fill_model=FillModel(),
            config=BacktestConfig())

        start = datetime(2013, 1, 1, 22, 2, 0, 0)
        stop = datetime(2013, 1, 1, 22, 5, 0, 0)

        # Act
        engine.run(start, stop)

        # Assert
        self.assertEqual(Timestamp('2013-01-01 00:00:00+00:00'), engine.data_client.execution_data_index_min)
        self.assertEqual(Timestamp('2013-12-31 23:59:00+00:00'), engine.data_client.execution_data_index_max)
        self.assertEqual(Tick, type(tick_tock.store[0]))
        self.assertEqual(Bar, type(tick_tock.store[1]))
        self.assertEqual(Tick, type(tick_tock.store[2]))
        self.assertEqual(TimeEvent, type(tick_tock.store[3]))
        self.assertEqual(TimeEvent, type(tick_tock.store[4]))
        self.assertEqual(Tick, type(tick_tock.store[5]))

    def test_timer_alert_sequencing_with_tick_execution(self):
        # Arrange
        instruments = [TestStubs.instrument_usdjpy()]
        tick_data = {USDJPY_FXCM: TestDataProvider.usdjpy_test_ticks()}
        bid_data = {USDJPY_FXCM: {Resolution.MINUTE: TestDataProvider.usdjpy_1min_bid()}}
        ask_data = {USDJPY_FXCM: {Resolution.MINUTE: TestDataProvider.usdjpy_1min_ask()}}

        instrument = TestStubs.instrument_usdjpy()
        bar_type = TestStubs.bartype_usdjpy_1min_bid()

        tick_tock = TickTock(instrument=instrument, bar_type=bar_type)

        engine = BacktestEngine(
            instruments=instruments,
            data_ticks=tick_data,
            data_bars_bid=bid_data,
            data_bars_ask=ask_data,
            strategies=[tick_tock],
            fill_model=FillModel(),
            config=BacktestConfig())

        start = datetime(2013, 1, 1, 22, 2, 0, 0)
        stop = datetime(2013, 1, 1, 22, 5, 0, 0)

        # Act
        engine.run(start, stop)

        # Assert
        self.assertEqual(Timestamp('2013-01-01 22:00:00.295000+00:00'), engine.data_client.execution_data_index_min)
        self.assertEqual(Timestamp('2013-01-01 22:35:13.494000+00:00'), engine.data_client.execution_data_index_max)

        print(tick_tock.store)
