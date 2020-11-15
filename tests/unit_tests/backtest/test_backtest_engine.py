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

from datetime import datetime
import unittest

from nautilus_trader.backtest.config import BacktestConfig
from nautilus_trader.backtest.data import BacktestDataContainer
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.loaders import InstrumentLoader
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import Venue
from tests.test_kit.data_provider import TestDataProvider
from tests.test_kit.strategies import EmptyStrategy
from tests.test_kit.stubs import TestStubs


USDJPY_FXCM = TestStubs.symbol_usdjpy_fxcm()


class BacktestEngineTests(unittest.TestCase):

    def setUp(self):
        usdjpy = InstrumentLoader.default_fx_ccy(TestStubs.symbol_usdjpy_fxcm())
        data = BacktestDataContainer()
        data.add_instrument(usdjpy)
        data.add_bars(usdjpy.symbol, BarAggregation.MINUTE, PriceType.BID, TestDataProvider.usdjpy_1min_bid()[:2000])
        data.add_bars(usdjpy.symbol, BarAggregation.MINUTE, PriceType.ASK, TestDataProvider.usdjpy_1min_ask()[:2000])

        self.engine = BacktestEngine(
            data=data,
            strategies=[EmptyStrategy("000")],
            venue=Venue("FXCM"),
            oms_type=OMSType.HEDGING,
            generate_position_ids=True,
            fill_model=FillModel(),
            config=BacktestConfig(),
        )

    def tearDown(self):
        self.engine.dispose()

    def test_initialization(self):
        self.assertEqual(1, len(self.engine.trader.strategy_states()))

    def test_reset_engine(self):
        # Arrange
        start = datetime(2013, 1, 1, 0, 0, 0, 0)
        stop = datetime(2013, 2, 1, 0, 0, 0, 0)

        self.engine.run(start, stop)

        # Act
        self.engine.reset()

        # Assert
        self.assertEqual(0, self.engine.iteration)  # No exceptions raised

    def test_run_empty_strategy(self):
        # Arrange
        start = datetime(2013, 1, 1, 0, 0, 0, 0)
        stop = datetime(2013, 2, 1, 0, 0, 0, 0)

        # Act
        self.engine.run(start, stop)

        # Assert
        self.assertEqual(4, self.engine.iteration)

    # TODO: New test
    # def test_timer_and_alert_sequencing_with_bar_execution(self):
    #     # Arrange
    #     usdjpy = InstrumentLoader.default_fx_ccy(TestStubs.symbol_usdjpy_fxcm())
    #     data = BacktestDataContainer()
    #     data.add_instrument(usdjpy)
    #     data.add_bars(usdjpy.symbol, BarAggregation.MINUTE, PriceType.BID, TestDataProvider.usdjpy_1min_bid()[:2000])
    #     data.add_bars(usdjpy.symbol, BarAggregation.MINUTE, PriceType.ASK, TestDataProvider.usdjpy_1min_ask()[:2000])
    #
    #     bar_type = TestStubs.bartype_usdjpy_1min_bid()
    #
    #     tick_tock = TickTock(instrument=usdjpy, bar_type=bar_type)
    #
    #     engine = BacktestEngine(
    #         data=data,
    #         strategies=[tick_tock],
    #         venue=Venue("FXCM"),
    #         oms_type=OMSType.HEDGING,
    #         generate_position_ids=True,
    #         fill_model=FillModel(),
    #         config=BacktestConfig(),
    #     )
    #
    #     start = datetime(2013, 1, 1, 22, 2, 0, 0)
    #     stop = datetime(2013, 1, 1, 22, 5, 0, 0)
    #
    #     # Act
    #     engine.run(start, stop)
    #
    #     # Assert
    #     self.assertEqual(Timestamp("2013-01-01 21:59:59.900000+0000"), engine.data_client.min_timestamp)
    #     self.assertEqual(Timestamp("2013-01-02 09:19:00+0000"), engine.data_client.max_timestamp)
    #     self.assertEqual([x.timestamp for x in tick_tock.store], sorted([x.timestamp for x in tick_tock.store]))  # Events in order

    # TODO: New test
    # def test_timer_alert_sequencing_with_tick_execution(self):
    #     # Arrange
    #     usdjpy = InstrumentLoader.default_fx_ccy(TestStubs.symbol_usdjpy_fxcm())
    #     data = BacktestDataContainer()
    #     data.add_instrument(usdjpy)
    #     data.add_bars(usdjpy.symbol, BarAggregation.MINUTE, PriceType.BID, TestDataProvider.usdjpy_1min_bid()[:2000])
    #     data.add_bars(usdjpy.symbol, BarAggregation.MINUTE, PriceType.ASK, TestDataProvider.usdjpy_1min_ask()[:2000])
    #
    #     bar_type = TestStubs.bartype_usdjpy_1min_bid()
    #
    #     tick_tock = TickTock(instrument=usdjpy, bar_type=bar_type)
    #
    #     engine = BacktestEngine(
    #         data=data,
    #         strategies=[tick_tock],
    #         venue=Venue("FXCM"),
    #         oms_type=OMSType.HEDGING,
    #         generate_position_ids=True,
    #         fill_model=FillModel(),
    #         config=BacktestConfig(),
    #     )
    #
    #     start = datetime(2013, 1, 1, 22, 2, 0, 0)
    #     stop = datetime(2013, 1, 1, 22, 5, 0, 0)
    #
    #     # Act
    #     engine.run(start, stop)
    #
    #     # Assert
    #     self.assertEqual(Timestamp("2013-01-01 21:59:59.900000+0000"), engine.data_client.min_timestamp)
    #     self.assertEqual(Timestamp("2013-01-02 09:19:00+0000"), engine.data_client.max_timestamp)
    #     self.assertEqual([x.timestamp for x in tick_tock.store], sorted([x.timestamp for x in tick_tock.store]))  # Events in order
