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


class BacktestEnginePerformanceTests(unittest.TestCase):

    def test_run_with_empty_strategy(self):
        # Arrange
        usdjpy = TestStubs.instrument_usdjpy()
        bid_data_1min = TestDataProvider.usdjpy_1min_bid()
        ask_data_1min = TestDataProvider.usdjpy_1min_ask()

        instruments = [TestStubs.instrument_usdjpy()]
        tick_data = {usdjpy.symbol: pd.DataFrame()}
        bid_data = {usdjpy.symbol: {Resolution.MINUTE: bid_data_1min}}
        ask_data = {usdjpy.symbol: {Resolution.MINUTE: ask_data_1min}}

        strategies = [EmptyStrategy()]

        config = BacktestConfig()
        engine = BacktestEngine(instruments=instruments,
                                data_ticks=tick_data,
                                data_bars_bid=bid_data,
                                data_bars_ask=ask_data,
                                strategies=strategies,
                                config=config)

        start = datetime(2013, 1, 1, 22, 0, 0, 0, tzinfo=timezone.utc)
        stop = datetime(2013, 8, 10, 0, 0, 0, 0, tzinfo=timezone.utc)

        cProfile.runctx('engine.run(start, stop)', globals(), locals(), 'Profile.prof')
        s = pstats.Stats("Profile.prof")
        s.strip_dirs().sort_stats("time").print_stats()

        self.assertTrue(True)

        # to datetime(2013, 8, 10, 0, 0, 0, 0, tzinfo=timezone.utc)
        #          3490226 function calls in 0.623 seconds
        #          5407539 function calls (5407535 primitive calls) in 1.187 seconds
        # 26/02/19 4450292 function calls (4450288 primitive calls) in 0.823 seconds

    def test_run_with_ema_cross_strategy(self):
        # Arrange
        usdjpy = TestStubs.instrument_usdjpy()
        bid_data_1min = TestDataProvider.usdjpy_1min_bid()
        ask_data_1min = TestDataProvider.usdjpy_1min_ask()

        instruments = [TestStubs.instrument_usdjpy()]
        tick_data = {usdjpy.symbol: pd.DataFrame()}
        bid_data = {usdjpy.symbol: {Resolution.MINUTE: bid_data_1min}}
        ask_data = {usdjpy.symbol: {Resolution.MINUTE: ask_data_1min}}

        strategies = [EMACross(label='001',
                               id_tag_trader='001',
                               id_tag_strategy='001',
                               instrument=usdjpy,
                               bar_type=TestStubs.bartype_usdjpy_1min_bid(),
                               risk_bp=10,
                               fast_ema=10,
                               slow_ema=20,
                               atr_period=20,
                               sl_atr_multiple=2.0)]

        config = BacktestConfig(slippage_ticks=1,
                                bypass_logging=True,
                                console_prints=False)
        engine = BacktestEngine(instruments=instruments,
                                data_ticks=tick_data,
                                data_bars_bid=bid_data,
                                data_bars_ask=ask_data,
                                strategies=strategies,
                                config=config)

        start = datetime(2013, 1, 1, 22, 0, 0, 0, tzinfo=timezone.utc)
        stop = datetime(2013, 3, 10, 0, 0, 0, 0, tzinfo=timezone.utc)

        cProfile.runctx('engine.run(start, stop)', globals(), locals(), 'Profile.prof')
        s = pstats.Stats("Profile.prof")
        s.strip_dirs().sort_stats("time").print_stats()

        # to datetime(2013, 3, 10, 0, 0, 0, 0, tzinfo=timezone.utc)
        #          51112051 function calls (51107866 primitive calls) in 22.586 seconds
        #          52912808 function calls (52908623 primitive calls) in 25.232 seconds
        #          49193278 function calls (49189089 primitive calls) in 19.121 seconds
        #          49193280 function calls (49189091 primitive calls) in 18.735 seconds
        #          42052320 function calls (42048131 primitive calls) in 17.642 seconds
        #          42098237 function calls (42094048 primitive calls) in 17.941 seconds
        #          39091577 function calls (39087388 primitive calls) in 16.455 seconds (removed price convenience method from build bars)
        #          21541910 function calls (21537721 primitive calls) in 11.654 seconds (added Price type)
        #          19915492 function calls (19911303 primitive calls) in 11.036 seconds
        #          22743234 function calls (22739045 primitive calls) in 12.844 seconds (implemented more sophisticated portfolio)
        # 31/01/19 22751226 function calls (22747037 primitive calls) in 12.830 seconds
        # 11/02/19 35533884 function calls (35533856 primitive calls) in 24.422 seconds (implemented concurrency)
        # 13/02/19 38049856 function calls (38049828 primitive calls) in 27.747 seconds
        # 15/02/19 45602587 function calls (45602559 primitive calls) in 32.350 seconds (introduced position events)
        # 24/02/19 40874448 function calls (40874420 primitive calls) in 31.250 seconds (separate threading between test and live)
        # 02/03/19 30212024 function calls (30212011 primitive calls) in 17.260 seconds (remove redundant Lock in id generation)

        self.assertTrue(True)
