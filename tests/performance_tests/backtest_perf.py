#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_backtest.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False

import cProfile
import pstats
import pandas as pd
import unittest

from datetime import datetime, timezone

from inv_trader.model.enums import Resolution
from inv_trader.model.enums import Venue
from inv_trader.model.objects import Symbol
from inv_trader.backtest.engine import BacktestConfig, BacktestEngine
from test_kit.strategies import EMACross
from test_kit.strategies import EmptyStrategy, EmptyStrategyCython
from test_kit.data import TestDataProvider
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
USDJPY_FXCM = Symbol('USDJPY', Venue.FXCM)


class BacktestEnginePerformanceTests(unittest.TestCase):

    @staticmethod
    def test_running_blank_strategy():
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

        start = datetime(2013, 1, 1, 22, 0, 0, 0, tzinfo=timezone.utc)
        stop = datetime(2013, 2, 10, 0, 0, 0, 0, tzinfo=timezone.utc)

        cProfile.runctx('engine.run(start, stop)', globals(), locals(), 'Profile.prof')
        s = pstats.Stats("Profile.prof")
        s.strip_dirs().sort_stats("time").print_stats()

    @staticmethod
    def test_can_run():
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

        config = BacktestConfig(console_prints=True)
        engine = BacktestEngine(instruments=instruments,
                                data_ticks=tick_data,
                                data_bars_bid=bid_data,
                                data_bars_ask=ask_data,
                                strategies=strategies,
                                config=config)

        start = datetime(2013, 1, 1, 22, 0, 0, 0, tzinfo=timezone.utc)
        stop = datetime(2013, 1, 10, 0, 0, 0, 0, tzinfo=timezone.utc)

        cProfile.runctx('engine.run(start, stop)', globals(), locals(), 'Profile.prof')
        s = pstats.Stats("Profile.prof")
        s.strip_dirs().sort_stats("time").print_stats()
