# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
from decimal import Decimal
import os
import pstats

import pandas as pd
import pytz

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.modules import FXRolloverInterestModule
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit import PACKAGE_ROOT
from tests.test_kit.performance import PerformanceHarness
from tests.test_kit.providers import TestDataProvider
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.strategies import EMACross
from tests.test_kit.stubs import TestStubs


USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")


class TestBacktestEnginePerformance(PerformanceHarness):
    @staticmethod
    def test_run_with_empty_strategy():
        # Arrange
        engine = BacktestEngine(bypass_logging=True)

        engine.add_instrument(USDJPY_SIM)
        engine.add_bars(
            USDJPY_SIM.id,
            BarAggregation.MINUTE,
            PriceType.BID,
            TestDataProvider.usdjpy_1min_bid(),
        )
        engine.add_bars(
            USDJPY_SIM.id,
            BarAggregation.MINUTE,
            PriceType.ASK,
            TestDataProvider.usdjpy_1min_ask(),
        )

        engine.add_venue(
            venue=Venue("SIM"),
            venue_type=VenueType.BROKERAGE,
            oms_type=OMSType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            fill_model=FillModel(),
        )

        strategies = [TradingStrategy("001")]

        start = datetime(2013, 1, 1, 22, 0, 0, 0, tzinfo=pytz.utc)
        stop = datetime(2013, 8, 10, 0, 0, 0, 0, tzinfo=pytz.utc)

        stats_file = "perf_stats_backtest_run_empty.prof"
        cProfile.runctx(
            "engine.run(start, stop, strategies=strategies)",
            globals(),
            locals(),
            stats_file,
        )
        s = pstats.Stats(stats_file)
        s.strip_dirs().sort_stats("time").print_stats()

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

    @staticmethod
    def test_run_for_tick_processing():
        # Arrange
        engine = BacktestEngine(bypass_logging=True)

        engine.add_instrument(USDJPY_SIM)
        engine.add_bars(
            USDJPY_SIM.id,
            BarAggregation.MINUTE,
            PriceType.BID,
            TestDataProvider.usdjpy_1min_bid(),
        )
        engine.add_bars(
            USDJPY_SIM.id,
            BarAggregation.MINUTE,
            PriceType.ASK,
            TestDataProvider.usdjpy_1min_ask(),
        )

        engine.add_venue(
            venue=Venue("SIM"),
            venue_type=VenueType.BROKERAGE,
            oms_type=OMSType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
        )

        strategy = EMACross(
            instrument_id=USDJPY_SIM.id,
            bar_spec=TestStubs.bar_spec_1min_bid(),
            trade_size=Decimal(1_000_000),
            fast_ema=10,
            slow_ema=20,
        )

        start = datetime(2013, 2, 1, 0, 0, 0, 0, tzinfo=pytz.utc)
        stop = datetime(2013, 2, 10, 0, 0, 0, 0, tzinfo=pytz.utc)

        stats_file = "perf_stats_tick_processing.prof"
        cProfile.runctx(
            "engine.run(start, stop, strategies=[strategy])",
            globals(),
            locals(),
            stats_file,
        )
        s = pstats.Stats(stats_file)
        s.strip_dirs().sort_stats("time").print_stats()

    @staticmethod
    def test_run_with_ema_cross_strategy():
        # Arrange
        engine = BacktestEngine(bypass_logging=True)

        engine.add_instrument(USDJPY_SIM)
        engine.add_bars(
            USDJPY_SIM.id,
            BarAggregation.MINUTE,
            PriceType.BID,
            TestDataProvider.usdjpy_1min_bid(),
        )
        engine.add_bars(
            USDJPY_SIM.id,
            BarAggregation.MINUTE,
            PriceType.ASK,
            TestDataProvider.usdjpy_1min_ask(),
        )

        interest_rate_data = pd.read_csv(
            os.path.join(PACKAGE_ROOT, "data", "short-term-interest.csv")
        )
        fx_rollover_interest = FXRolloverInterestModule(rate_data=interest_rate_data)

        engine.add_venue(
            venue=Venue("SIM"),
            venue_type=VenueType.BROKERAGE,
            oms_type=OMSType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            modules=[fx_rollover_interest],
        )

        strategy = EMACross(
            instrument_id=USDJPY_SIM.id,
            bar_spec=TestStubs.bar_spec_1min_bid(),
            trade_size=Decimal(1_000_000),
            fast_ema=10,
            slow_ema=20,
        )

        start = datetime(2013, 2, 1, 0, 0, 0, 0, tzinfo=pytz.utc)
        stop = datetime(2013, 3, 1, 0, 0, 0, 0, tzinfo=pytz.utc)

        stats_file = "perf_stats_backtest_run_ema.prof"
        cProfile.runctx(
            "engine.run(start, stop, strategies=[strategy])",
            globals(),
            locals(),
            stats_file,
        )
        s = pstats.Stats(stats_file)
        s.strip_dirs().sort_stats("time").print_stats()

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
        # 25/10/20  4345869 function calls (4320566 primitive calls) in 5.519 seconds (improved efficiency of analyzers)
        # 28/10/20  4345843 function calls (4320540 primitive calls) in 5.964 seconds (implement @property were appropriate)
        # 29/10/20  4305523 function calls (4280220 primitive calls) in 5.876 seconds (simplifications)
        # 01/11/20  4305523 function calls (4280220 primitive calls) in 5.499 seconds (reinstate some readonly attributes)
        # 07/11/20  4345844 function calls (4320541 primitive calls) in 6.051 seconds (performance check)
        # 08/11/20  4345844 function calls (4320541 primitive calls) in 5.960 seconds (centralize exchange rate calculations)
        # 12/11/20  4346944 function calls (4321640 primitive calls) in 5.809 seconds (change value object API)
        # 15/11/20  4378755 function calls (4353255 primitive calls) in 5.927 seconds (performance check)
        # 18/11/20  4377984 function calls (4352279 primitive calls) in 5.908 seconds (performance check)
        # 25/11/20  4394122 function calls (4363978 primitive calls) in 6.212 seconds (performance check)
        # 27/11/20  4294514 function calls (4268761 primitive calls) in 5.822 seconds (remove redundant methods)
        # 29/11/20  4374015 function calls (4348306 primitive calls) in 5.753 seconds (performance check)
        # 09/12/20  4294769 function calls (4268911 primitive calls) in 5.858 seconds (performance check)
        # 14/12/20  5685767 function calls (5648057 primitive calls) in 6.484 seconds (multi-currency accounts)
        # 01/01/21  5657521 function calls (5615526 primitive calls) in 6.960 seconds (performance check)
        # 03/01/21  5518555 function calls (5480845 primitive calls) in 6.529 seconds (make handlers c methods)
        # 31/01/21  5408449 function calls (5370737 primitive calls) in 6.890 seconds (refactor execution engine)
        # 31/01/21  5410257 function calls (5372573 primitive calls) in 7.611 seconds (performance check)
        # 04/02/21  5405862 function calls (5368174 primitive calls) in 7.359 seconds (performance check)
        # 04/02/21  5405726 function calls (5368038 primitive calls) in 7.196 seconds (performance check)
        # 24/04/21  5375115 function calls (5337643 primitive calls) in 10.553 seconds (order book in exchanges)
        # 24/04/21  5375115 function calls (5337643 primitive calls) in 10.009 seconds (order book optimizations)
        # 26/04/21  5405727 function calls (5368039 primitive calls) in 7.469 seconds (order book optimizations)
        # 01/05/21  5405727 function calls (5368039 primitive calls) in 7.533 seconds (refactorings)
        # 22/05/21  5517969 function calls (5479092 primitive calls) in 8.639 seconds (rewire risk engine)
        # 25/05/21  5517969 function calls (5479092 primitive calls) in 8.387 seconds (rewrite account states)
        # 02/07/21  5504684 function calls (5467951 primitive calls) in 8.731 seconds (performance check)
