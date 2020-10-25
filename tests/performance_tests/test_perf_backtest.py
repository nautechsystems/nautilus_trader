# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import cProfile
from datetime import datetime
import pstats
import unittest

import pytz

from nautilus_trader.backtest.config import BacktestConfig
from nautilus_trader.backtest.data import BacktestDataContainer
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.loaders import InstrumentLoader
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import Venue
from tests.test_kit.data import TestDataProvider
from tests.test_kit.strategies import EMACross
from tests.test_kit.strategies import EmptyStrategy
from tests.test_kit.stubs import TestStubs

USDJPY_FXCM = TestStubs.symbol_usdjpy_fxcm()


class BacktestEnginePerformanceTests(unittest.TestCase):

    def test_run_with_empty_strategy(self):
        # Arrange
        usdjpy = InstrumentLoader.default_fx_ccy(TestStubs.symbol_usdjpy_fxcm())

        data = BacktestDataContainer()
        data.add_instrument(usdjpy)
        data.add_bars(usdjpy.symbol, BarAggregation.MINUTE, PriceType.BID, TestDataProvider.usdjpy_1min_bid())
        data.add_bars(usdjpy.symbol, BarAggregation.MINUTE, PriceType.ASK, TestDataProvider.usdjpy_1min_ask())

        strategies = [EmptyStrategy("001")]

        config = BacktestConfig(exec_db_type="in-memory")
        engine = BacktestEngine(
            data=data,
            strategies=strategies,
            venue=Venue("FXCM"),
            oms_type=OMSType.HEDGING,
            generate_position_ids=True,
            fill_model=FillModel(),
            config=config,
        )

        start = datetime(2013, 1, 1, 22, 0, 0, 0, tzinfo=pytz.utc)
        stop = datetime(2013, 8, 10, 0, 0, 0, 0, tzinfo=pytz.utc)

        stats_file = "perf_stats_backtest_run_empty.prof"
        cProfile.runctx("engine.run(start, stop)", globals(), locals(), stats_file)
        s = pstats.Stats(stats_file)
        s.strip_dirs().sort_stats("time").print_stats()

        self.assertTrue(True)

        # to datetime(2013, 8, 10, 0, 0, 0, 0, tzinfo=pytz.utc)
        #          3490226 function calls in 0.623 seconds
        #          5407539 function calls (5407535 primitive calls) in 1.187 seconds
        # 26/02/19 4450292 function calls (4450288 primitive calls) in 0.823 seconds
        # 08/03/19    5194 function calls    (5194 primitive calls) in 0.167 seconds (removed performance hooks)
        # 13/03/19    8691 function calls    (8613 primitive calls) in 0.162 seconds (added analyzer)
        # 13/03/19  614197 function calls  (614015 primitive calls) in 0.694 seconds (numerous changes)
        # 16/03/19 2193923 function calls (2193741 primitive calls) in 2.690 seconds (changed)
        # 27/03/19 2255252 function calls (2255070 primitive calls) in 2.738 seconds (performance check)
        # 09/07/19   78020 function calls   (77838 primitive calls) in 2.179 seconds (performance check)
        # 31/07/19   13792 function calls   (13610 primitive calls) in 2.037 seconds (performance check)
        # 21/08/19   15311 function calls   (15117 primitive calls) in 2.156 seconds (performance check)
        # 14/01/20   20964 function calls   (20758 primitive calls) in 0.695 seconds (performance check)
        # 10/02/20   713938 function calls (713572 primitive calls) in 2.670 seconds (something changed)

    def test_run_for_tick_processing(self):
        # Arrange
        usdjpy = InstrumentLoader.default_fx_ccy(TestStubs.symbol_usdjpy_fxcm())

        data = BacktestDataContainer()
        data.add_instrument(usdjpy)
        data.add_bars(usdjpy.symbol, BarAggregation.MINUTE, PriceType.BID, TestDataProvider.usdjpy_1min_bid())
        data.add_bars(usdjpy.symbol, BarAggregation.MINUTE, PriceType.ASK, TestDataProvider.usdjpy_1min_ask())

        strategies = [EMACross(
            symbol=usdjpy.symbol,
            bar_spec=TestStubs.bar_spec_1min_bid(),
            fast_ema=10,
            slow_ema=20)]

        config = BacktestConfig(
            exec_db_type="in-memory",
            bypass_logging=True,
            console_prints=False)

        engine = BacktestEngine(
            data=data,
            strategies=strategies,
            venue=Venue("FXCM"),
            oms_type=OMSType.HEDGING,
            generate_position_ids=True,
            config=config,
            fill_model=None,
        )

        start = datetime(2013, 2, 1, 0, 0, 0, 0, tzinfo=pytz.utc)
        stop = datetime(2013, 2, 10, 0, 0, 0, 0, tzinfo=pytz.utc)

        stats_file = "perf_stats_tick_processing.prof"
        cProfile.runctx("engine.run(start, stop)", globals(), locals(), stats_file)
        s = pstats.Stats(stats_file)
        s.strip_dirs().sort_stats("time").print_stats()

        self.assertTrue(True)

    def test_run_with_ema_cross_strategy(self):
        # Arrange
        usdjpy = InstrumentLoader.default_fx_ccy(TestStubs.symbol_usdjpy_fxcm())

        data = BacktestDataContainer()
        data.add_instrument(usdjpy)
        data.add_bars(usdjpy.symbol, BarAggregation.MINUTE, PriceType.BID, TestDataProvider.usdjpy_1min_bid())
        data.add_bars(usdjpy.symbol, BarAggregation.MINUTE, PriceType.ASK, TestDataProvider.usdjpy_1min_ask())

        strategies = [EMACross(
            symbol=usdjpy.symbol,
            bar_spec=TestStubs.bar_spec_1min_bid(),
            fast_ema=10,
            slow_ema=20)]

        config = BacktestConfig(
            exec_db_type="in-memory",
            bypass_logging=True,
            console_prints=False)

        engine = BacktestEngine(
            data=data,
            strategies=strategies,
            venue=Venue("FXCM"),
            oms_type=OMSType.HEDGING,
            generate_position_ids=True,
            config=config,
            fill_model=None,
        )

        start = datetime(2013, 2, 1, 0, 0, 0, 0, tzinfo=pytz.utc)
        stop = datetime(2013, 3, 1, 0, 0, 0, 0, tzinfo=pytz.utc)

        stats_file = "perf_stats_backtest_run_ema.prof"
        cProfile.runctx("engine.run(start, stop)", globals(), locals(), stats_file)
        s = pstats.Stats(stats_file)
        s.strip_dirs().sort_stats("time").print_stats()

        self.assertTrue(True)

        # 05/10/20 Change to simple EMA Cross - 1 month.
        # start = datetime(2013, 2, 1, 0, 0, 0, 0, tzinfo=pytz.utc)
        # stop = datetime(2013, 3, 1, 0, 0, 0, 0, tzinfo=pytz.utc)
        # -----------------------------------------------------------------------------------------------------------------------------------------
        # 08/10/20 9502337 function calls  (9417247 primitive calls) in 10.532 seconds (benchmark)
        # 12/10/20 14520673 function calls (14405965 primitive calls) in 13.248 seconds (wire portfolio to receive ticks)
        # 13/10/20 14502141 function calls (14387433 primitive calls) in 14.062 seconds (build out account and portfolio updates)
        # 14/10/20 14329004 function calls (14214466 primitive calls) in 12.796 seconds (improve portfolio efficiency)
        # 15/10/20 14438290 function calls (14323752 primitive calls) in 13.273 seconds (various refactorings for clarity)
        # 19/10/20 10880065 function calls (10792051 primitive calls) in 11.181 seconds (performance optimizations)
        # 20/10/20 14130784 function calls (14016236 primitive calls) in 12.892 seconds (replace Fraction with wrapped Decimal)
        # 21/10/20 21651240 function calls (21484376 primitive calls) in 19.456 seconds (more robust data engine for uvloop - will optimize)
        # 22/10/20 21635544 function calls (21468680 primitive calls) in 18.997 seconds (data wrapper classes)
        # 22/10/20 21635544 function calls (21468680 primitive calls) in 18.759 seconds (cdef inline data handler methods)
        # 22/10/20 20505398 function calls (20338534 primitive calls) in 17.905 seconds (remove redundant precision calculation)
        # 22/10/20 20505398 function calls (20338534 primitive calls) in 14.170 seconds (improve portfolio calculation efficiency)
        # 25/10/20  4345859 function calls (4320560 primitive calls) in 5.647 seconds (improved efficiency of analyzers)
