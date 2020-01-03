# -------------------------------------------------------------------------------------------------
# <copyright file="test_backtest_engine.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import pandas as pd
import unittest

from datetime import datetime
from pandas import Timestamp

from nautilus_trader.model.enums import BarStructure
from nautilus_trader.model.objects import Tick, Bar
from nautilus_trader.model.events import TimeEvent
from nautilus_trader.backtest.config import BacktestConfig
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.engine import BacktestEngine
from test_kit.strategies import EmptyStrategy, EMACross, TickTock
from test_kit.data import TestDataProvider
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
USDJPY_FXCM = TestStubs.symbol_usdjpy_fxcm()

# TODO: Speed these tests up


class BacktestEngineTests(unittest.TestCase):

    def setUp(self):
        instruments = [TestStubs.instrument_usdjpy()]
        tick_data = {USDJPY_FXCM: pd.DataFrame()}
        bid_data = {USDJPY_FXCM: {BarStructure.MINUTE: TestDataProvider.usdjpy_1min_bid()}}
        ask_data = {USDJPY_FXCM: {BarStructure.MINUTE: TestDataProvider.usdjpy_1min_ask()}}

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
        self.assertEqual(1, len(self.engine.trader.strategy_status()))

    def test_timer_and_alert_sequencing_with_bar_execution(self):
        # Arrange
        instruments = [TestStubs.instrument_usdjpy()]
        tick_data = {USDJPY_FXCM: pd.DataFrame()}
        bid_data = {USDJPY_FXCM: {BarStructure.MINUTE: TestDataProvider.usdjpy_1min_bid()}}
        ask_data = {USDJPY_FXCM: {BarStructure.MINUTE: TestDataProvider.usdjpy_1min_ask()}}

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
        self.assertEqual([x.timestamp for x in tick_tock.store], sorted([x.timestamp for x in tick_tock.store]))  # Events in order

    def test_timer_alert_sequencing_with_tick_execution(self):
        # Arrange
        instruments = [TestStubs.instrument_usdjpy()]
        tick_data = {USDJPY_FXCM: TestDataProvider.usdjpy_test_ticks()}
        bid_data = {USDJPY_FXCM: {BarStructure.MINUTE: TestDataProvider.usdjpy_1min_bid()}}
        ask_data = {USDJPY_FXCM: {BarStructure.MINUTE: TestDataProvider.usdjpy_1min_ask()}}

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
        self.assertEqual([x.timestamp for x in tick_tock.store], sorted([x.timestamp for x in tick_tock.store]))  # Events in order
