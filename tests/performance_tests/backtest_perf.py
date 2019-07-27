# -------------------------------------------------------------------------------------------------
# <copyright file="test_backtest.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import cProfile
import pstats
import pandas as pd
import unittest

from datetime import datetime, timezone

from nautilus_trader.model.enums import Resolution
from nautilus_trader.model.enums import Venue
from nautilus_trader.model.objects import Symbol
from nautilus_trader.backtest.config import BacktestConfig
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.models import FillModel
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

        strategies = [EmptyStrategy('001')]

        config = BacktestConfig()
        engine = BacktestEngine(instruments=instruments,
                                data_ticks=tick_data,
                                data_bars_bid=bid_data,
                                data_bars_ask=ask_data,
                                strategies=strategies,
                                fill_model=FillModel(),
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
        # 08/03/19    5194 function calls    (5194 primitive calls) in 0.167 seconds (removed performance hooks)
        # 13/03/19    8691 function calls    (8613 primitive calls) in 0.162 seconds (added analyzer)
        # 13/03/19  614197 function calls  (614015 primitive calls) in 0.694 seconds (numerous changes)
        # 16/03/19 2193923 function calls (2193741 primitive calls) in 2.690 seconds (changed)
        # 27/03/19 2255252 function calls (2255070 primitive calls) in 2.738 seconds (performance check)
        # 09/07/19   78020 function calls (77838 primitive calls) in 2.179 seconds (performance check)

    def test_run_with_ema_cross_strategy(self):
        # Arrange
        usdjpy = TestStubs.instrument_usdjpy()
        bid_data_1min = TestDataProvider.usdjpy_1min_bid()
        ask_data_1min = TestDataProvider.usdjpy_1min_ask()

        instruments = [TestStubs.instrument_usdjpy()]
        tick_data = {usdjpy.symbol: pd.DataFrame()}
        bid_data = {usdjpy.symbol: {Resolution.MINUTE: bid_data_1min}}
        ask_data = {usdjpy.symbol: {Resolution.MINUTE: ask_data_1min}}

        strategies = [EMACross(
            instrument=usdjpy,
            bar_type=TestStubs.bartype_usdjpy_1min_bid(),
            risk_bp=10,
            fast_ema=10,
            slow_ema=20,
            atr_period=20,
            sl_atr_multiple=2.0)]

        fill_model = FillModel(
            prob_fill_at_limit=0.2,
            prob_fill_at_stop=0.95,
            prob_slippage=0.5)

        config = BacktestConfig(
            bypass_logging=True,
            console_prints=False)

        engine = BacktestEngine(
            instruments=instruments,
            data_ticks=tick_data,
            data_bars_bid=bid_data,
            data_bars_ask=ask_data,
            strategies=strategies,
            fill_model=FillModel(),
            config=config)

        start = datetime(2013, 1, 1, 22, 0, 0, 0, tzinfo=timezone.utc)
        stop = datetime(2013, 3, 10, 0, 0, 0, 0, tzinfo=timezone.utc)

        cProfile.runctx('engine.run(start, stop)', globals(), locals(), 'Profile.prof')
        s = pstats.Stats("Profile.prof")
        s.strip_dirs().sort_stats("time").print_stats()

        self.assertTrue(True)

        # start datetime(2013, 1, 1, 22, 0, 0, 0, tzinfo=timezone.utc)
        # stop  datetime(2013, 3, 10, 0, 0, 0, 0, tzinfo=timezone.utc)
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
        # 02/03/19 30638417 function calls (30638404 primitive calls) in 18.212 seconds (add max trade size limiter)
        # 02/03/19  4521539 function calls  (4521526 primitive calls) in 10.337 seconds (with no profile hooks!)
        # 03/03/19  6809241 function calls  (6809223 primitive calls) in 13.887 seconds (added tick iterations)
        # 04/03/19  6809285 function calls  (6809267 primitive calls) in 13.740 seconds (add return calculations to positions)
        # 08/03/19  9249474 function calls  (9210052 primitive calls) in 16.686 seconds (add portfolio analysis)
        # 11/03/19  9352466 function calls  (9312764 primitive calls) in 16.185 seconds (add more portfolio analysis)
        # 11/03/19  9262774 function calls  (9223072 primitive calls) in 14.489 seconds (append left to bars list)
        # 13/03/19  6111078 function calls  (6071325 primitive calls) in 13.208 seconds (improve loops)
        # 15/03/19  9352490 function calls  (9312786 primitive calls) in 16.069 seconds (add position calculations)
        # 19/03/19  9352531 function calls  (9312827 primitive calls) in 16.249 seconds (perf check)
        # 20/03/19  9352531 function calls  (9312827 primitive calls) in 15.544 seconds (perf check)
        # 25/03/19  9352619 function calls  (9312915 primitive calls) in 16.268 seconds (more detailed transaction and commission calculations)
        # 26/03/19  8975755 function calls  (8941827 primitive calls) in 16.211 seconds (improve backtest execution)
        # 02/04/19  9189830 function calls  (9152734 primitive calls) in 17.560 seconds (added OCO order handling)
        # 03/04/19  9189830 function calls  (9152734 primitive calls) in 17.493 seconds (added enhanced exchange rate calculations)
        # 09/04/19  8418269 function calls  (8395079 primitive calls) in 19.168 seconds (removed redundant deque processing for backtest execution)
        # --------------------------------------------------------------------------------------------------------------------------------------------------------------------
        # 10/04/19 27162891 function calls (26849875 primitive calls) in 27.461 seconds (after fixing bug strategy is processing properly with many more orders and positions)
        # 11/04/19 27094455 function calls (26782489 primitive calls) in 26.730 seconds (even after enhancing execution detail)
        # 11/04/19 27094455 function calls (26782489 primitive calls) in 57.559 seconds (new order registration has slowed things down a lot)
        # 13/04/19 27087701 function calls (26775735 primitive calls) in 28.216 seconds (found bugs in execution, cleaned up residual objects)
        # 13/04/19 27203159 function calls (26890143 primitive calls) in 28.170 seconds (with fill modelling)
        # 16/04/19 20079206 function calls (19785106 primitive calls) in 17.252 seconds (improved backtest loop!)
        # 17/04/19 20079230 function calls (19785130 primitive calls) in 17.806 seconds (added log store)
        # 18/04/19 20079246 function calls (19785146 primitive calls) in 17.217 seconds (changed order and position objects)
        # 20/04/19 20124704 function calls (19830594 primitive calls) in 17.214 seconds (improve data base class)
        # 21/04/19 15806081 function calls (15577303 primitive calls) in 14.045 seconds (new analyzers reduce number of trades)
        # 22/04/19 15759862 function calls (15531098 primitive calls) in 13.918 seconds (redo id generation)
        # 24/04/19 15786882 function calls (15558362 primitive calls) in 13.655 seconds (improve backtest loops)
        # 25/04/19 15788185 function calls (15559675 primitive calls) in 14.044 seconds (performance check)
        # 26/04/19 15741199 function calls (15512699 primitive calls) in 14.054 seconds (extra data integrity checks)
        # 27/04/19 15175849 function calls (14947473 primitive calls) in 13.746 seconds (performance check)
        # 28/04/19 15175781 function calls (14947405 primitive calls) in 15.073 seconds (increase resolution of time events)
        # 29/04/19 15128798 function calls (14900430 primitive calls) in 14.235 seconds (add bid-ask bar pair class)
        # 09/07/19 15323948 function calls (15095596 primitive calls) in 14.408 seconds (performance check)
        # 25/07/19 15274063 function calls (15045703 primitive calls) in 14.408 seconds (removed compiler directives for perf)
