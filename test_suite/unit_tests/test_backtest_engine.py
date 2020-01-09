# -------------------------------------------------------------------------------------------------
# <copyright file="test_backtest_engine.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from datetime import datetime
from pandas import Timestamp

from nautilus_trader.model.enums import BarStructure, PriceType
from nautilus_trader.backtest.data import BacktestDataContainer
from nautilus_trader.backtest.config import BacktestConfig
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.engine import BacktestEngine
from test_kit.strategies import EmptyStrategy, TickTock
from test_kit.data import TestDataProvider
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
USDJPY_FXCM = TestStubs.symbol_usdjpy_fxcm()


class BacktestEngineTests(unittest.TestCase):

    def setUp(self):
        usdjpy = TestStubs.instrument_usdjpy()
        data = BacktestDataContainer()
        data.add_instrument(usdjpy)
        data.add_bars(usdjpy.symbol, BarStructure.MINUTE, PriceType.BID, TestDataProvider.usdjpy_1min_bid()[:2000])
        data.add_bars(usdjpy.symbol, BarStructure.MINUTE, PriceType.ASK, TestDataProvider.usdjpy_1min_ask()[:2000])

        self.engine = BacktestEngine(
            data=data,
            strategies=[EmptyStrategy('000')],
            fill_model=FillModel(),
            config=BacktestConfig())

    def tearDown(self):
        self.engine.dispose()

    def test_initialization(self):
        self.assertEqual(1, len(self.engine.trader.strategy_status()))

    def test_timer_and_alert_sequencing_with_bar_execution(self):
        # Arrange
        usdjpy = TestStubs.instrument_usdjpy()
        data = BacktestDataContainer()
        data.add_instrument(usdjpy)
        data.add_bars(usdjpy.symbol, BarStructure.MINUTE, PriceType.BID, TestDataProvider.usdjpy_1min_bid()[:2000])
        data.add_bars(usdjpy.symbol, BarStructure.MINUTE, PriceType.ASK, TestDataProvider.usdjpy_1min_ask()[:2000])

        instrument = TestStubs.instrument_usdjpy()
        bar_type = TestStubs.bartype_usdjpy_1min_bid()

        tick_tock = TickTock(instrument=instrument, bar_type=bar_type)

        engine = BacktestEngine(
            data=data,
            strategies=[tick_tock],
            fill_model=FillModel(),
            config=BacktestConfig())

        start = datetime(2013, 1, 1, 22, 2, 0, 0)
        stop = datetime(2013, 1, 1, 22, 5, 0, 0)

        # Act
        engine.run(start, stop)

        # Assert
        self.assertEqual(Timestamp('2013-01-01 21:59:59.900000+0000'), engine.data_client.min_timestamp)
        self.assertEqual(Timestamp('2013-01-02 09:19:00+0000'), engine.data_client.max_timestamp)
        #self.assertEqual([x.timestamp for x in tick_tock.store], sorted([x.timestamp for x in tick_tock.store]))  # Events in order

    def test_timer_alert_sequencing_with_tick_execution(self):
        # Arrange
        usdjpy = TestStubs.instrument_usdjpy()
        data = BacktestDataContainer()
        data.add_instrument(usdjpy)
        data.add_bars(usdjpy.symbol, BarStructure.MINUTE, PriceType.BID, TestDataProvider.usdjpy_1min_bid()[:2000])
        data.add_bars(usdjpy.symbol, BarStructure.MINUTE, PriceType.ASK, TestDataProvider.usdjpy_1min_ask()[:2000])

        bar_type = TestStubs.bartype_usdjpy_1min_bid()

        tick_tock = TickTock(instrument=usdjpy, bar_type=bar_type)

        engine = BacktestEngine(
            data=data,
            strategies=[tick_tock],
            fill_model=FillModel(),
            config=BacktestConfig())

        start = datetime(2013, 1, 1, 22, 2, 0, 0)
        stop = datetime(2013, 1, 1, 22, 5, 0, 0)

        # Act
        engine.run(start, stop)

        # Assert
        self.assertEqual(Timestamp('2013-01-01 21:59:59.900000+0000'), engine.data_client.min_timestamp)
        self.assertEqual(Timestamp('2013-01-02 09:19:00+0000'), engine.data_client.max_timestamp)
        #self.assertEqual([x.timestamp for x in tick_tock.store], sorted([x.timestamp for x in tick_tock.store]))  # Events in order
