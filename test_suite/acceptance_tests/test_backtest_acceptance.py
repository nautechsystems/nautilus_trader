# -------------------------------------------------------------------------------------------------
# <copyright file="test_backtest_acceptance.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import pandas as pd
import unittest

from datetime import datetime

from nautilus_trader.model.enums import Resolution
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.backtest.config import BacktestConfig
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.engine import BacktestEngine
from test_kit.strategies import EmptyStrategy, EMACross
from test_kit.data import TestDataProvider
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
USDJPY_FXCM = TestStubs.symbol_usdjpy_fxcm()


class BacktestAcceptanceTests(unittest.TestCase):

    def setUp(self):
        instruments = [TestStubs.instrument_usdjpy()]
        tick_data = {USDJPY_FXCM: pd.DataFrame()}
        bid_data = {USDJPY_FXCM: {Resolution.MINUTE: TestDataProvider.usdjpy_1min_bid()}}
        ask_data = {USDJPY_FXCM: {Resolution.MINUTE: TestDataProvider.usdjpy_1min_ask()}}

        self.engine = BacktestEngine(
            trader_id=None,
            venue=Venue('FXCM'),
            instruments=instruments,
            data_ticks=tick_data,
            data_bars_bid=bid_data,
            data_bars_ask=ask_data,
            strategies=[EmptyStrategy('000')],
            fill_model=FillModel(),
            config=BacktestConfig())

    def tearDown(self):
        self.engine.dispose()

    def test_can_run_empty_strategy(self):
        # Arrange
        start = datetime(2013, 1, 1, 0, 0, 0, 0)
        stop = datetime(2013, 2, 1, 0, 0, 0, 0)

        # Act
        self.engine.run(start, stop)

        # Assert
        self.assertEqual(44641, self.engine.iteration)

    def test_can_reset_engine(self):
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
        self.assertEqual(-12085.9296875, self.engine.get_performance_stats()['PNL'])  # Money represented as float here

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
