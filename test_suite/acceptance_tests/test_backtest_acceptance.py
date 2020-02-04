# -------------------------------------------------------------------------------------------------
# <copyright file="test_backtest_acceptance.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
from datetime import datetime

from nautilus_trader.common.logger import LogLevel
from nautilus_trader.model.enums import BarStructure, PriceType, Currency
from nautilus_trader.backtest.data import BacktestDataContainer
from nautilus_trader.backtest.config import BacktestConfig
from nautilus_trader.backtest.engine import BacktestEngine
from test_kit.strategies import EmptyStrategy, EMACross
from test_kit.data import TestDataProvider
from test_kit.stubs import TestStubs

USDJPY_FXCM = TestStubs.symbol_usdjpy_fxcm()


class BacktestAcceptanceTests(unittest.TestCase):

    def setUp(self):
        self.usdjpy = TestStubs.instrument_usdjpy()
        data = BacktestDataContainer()
        data.add_instrument(self.usdjpy)
        data.add_bars(self.usdjpy.symbol, BarStructure.MINUTE, PriceType.BID, TestDataProvider.usdjpy_1min_bid()[:2000])
        data.add_bars(self.usdjpy.symbol, BarStructure.MINUTE, PriceType.ASK, TestDataProvider.usdjpy_1min_ask()[:2000])

        config = BacktestConfig(
            exec_db_type='in-memory',
            exec_db_flush=False,
            frozen_account=False,
            starting_capital=1000000,
            account_currency=Currency.USD,
            short_term_interest_csv_path='default',
            commission_rate_bp=0.20,
            bypass_logging=True,
            level_console=LogLevel.DEBUG,
            level_file=LogLevel.DEBUG,
            level_store=LogLevel.WARNING,
            log_thread=False,
            log_to_file=False)

        self.engine = BacktestEngine(
            data=data,
            strategies=[EmptyStrategy('000')],
            config=config)

    def tearDown(self):
        self.engine.dispose()

    def test_can_run_empty_strategy(self):
        # Arrange
        start = datetime(2013, 1, 1, 0, 0, 0, 0)
        stop = datetime(2013, 2, 1, 0, 0, 0, 0)

        # Act
        self.engine.run(start, stop)

        # Assert
        self.assertEqual(2040, self.engine.iteration)

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
        strategies = [EMACross(instrument=self.usdjpy,
                               bar_spec=TestStubs.bar_spec_1min_bid(),
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
        self.assertEqual(559, strategies[0].fast_ema.count)
        self.assertEqual(-1052.65, self.engine.analyzer.get_performance_stats()['PNL'])  # Money represented as double here

    def test_can_rerun_ema_cross_strategy_returns_identical_performance(self):
        # Arrange
        strategies = [EMACross(instrument=self.usdjpy,
                               bar_spec=TestStubs.bar_spec_1min_bid(),
                               risk_bp=10,
                               fast_ema=10,
                               slow_ema=20,
                               atr_period=20,
                               sl_atr_multiple=2.0)]

        start = datetime(2013, 1, 2, 0, 0, 0, 0)
        stop = datetime(2013, 1, 5, 0, 0, 0, 0)

        self.engine.run(start, stop, strategies=strategies)
        result1 = self.engine.analyzer.get_performance_stats()

        # Act
        self.engine.run(start, stop)
        result2 = self.engine.analyzer.get_performance_stats()

        # Assert
        self.assertEqual(all(result1), all(result2))

    def test_can_run_multiple_strategies(self):
        # Arrange
        strategies = [EMACross(instrument=self.usdjpy,
                               bar_spec=TestStubs.bar_spec_1min_bid(),
                               risk_bp=10,
                               fast_ema=10,
                               slow_ema=20,
                               atr_period=20,
                               sl_atr_multiple=2.0,
                               extra_id_tag='001'),
                      EMACross(instrument=self.usdjpy,
                               bar_spec=TestStubs.bar_spec_1min_bid(),
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
        self.assertEqual(559, strategies[0].fast_ema.count)
        self.assertEqual(559, strategies[1].fast_ema.count)
