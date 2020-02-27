# -------------------------------------------------------------------------------------------------
# <copyright file="test_trading_trader.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from nautilus_trader.analysis.performance import PerformanceAnalyzer
from nautilus_trader.model.enums import BarStructure, PriceType, Currency
from nautilus_trader.model.identifiers import IdTag, TraderId, StrategyId
from nautilus_trader.common.guid import TestGuidFactory
from nautilus_trader.common.logging import TestLogger
from nautilus_trader.common.execution import ExecutionEngine, InMemoryExecutionDatabase
from nautilus_trader.common.portfolio import Portfolio
from nautilus_trader.common.clock import TestClock
from nautilus_trader.backtest.config import BacktestConfig
from nautilus_trader.backtest.execution import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.data import BacktestDataContainer, BacktestDataClient
from nautilus_trader.trading.trader import Trader
from test_kit.strategies import EmptyStrategy
from test_kit.stubs import TestStubs
from test_kit.data import TestDataProvider

USDJPY_FXCM = TestStubs.instrument_usdjpy().symbol


class TraderTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        usdjpy = TestStubs.instrument_usdjpy()
        data = BacktestDataContainer()
        data.add_instrument(usdjpy)
        data.add_bars(usdjpy.symbol, BarStructure.MINUTE, PriceType.BID, TestDataProvider.usdjpy_1min_bid()[:2000])
        data.add_bars(usdjpy.symbol, BarStructure.MINUTE, PriceType.ASK, TestDataProvider.usdjpy_1min_ask()[:2000])

        clock = TestClock()
        guid_factory = TestGuidFactory()
        logger = TestLogger()
        trader_id = TraderId('TESTER', '000')
        account_id = TestStubs.account_id()

        data_client = BacktestDataClient(
            data=data,
            tick_capacity=100,
            clock=clock,
            logger=logger)

        self.portfolio = Portfolio(
            currency=Currency.USD,
            clock=clock,
            guid_factory=guid_factory,
            logger=logger)

        self.analyzer = PerformanceAnalyzer()

        self.exec_db = InMemoryExecutionDatabase(
            trader_id=trader_id,
            logger=logger)
        self.exec_engine = ExecutionEngine(
            trader_id=trader_id,
            account_id=account_id,
            database=self.exec_db,
            portfolio=self.portfolio,
            clock=clock,
            guid_factory=guid_factory,
            logger=logger)

        self.exec_client = BacktestExecClient(
            exec_engine=self.exec_engine,
            instruments={usdjpy.symbol: usdjpy},
            config=BacktestConfig(),
            fill_model=FillModel(),
            clock=clock,
            guid_factory=guid_factory,
            logger=logger)
        self.exec_engine.register_client(self.exec_client)

        strategies = [EmptyStrategy('001'),
                      EmptyStrategy('002')]

        self.trader = Trader(
            trader_id=trader_id,
            account_id=account_id,
            strategies=strategies,
            data_client=data_client,
            exec_engine=self.exec_engine,
            clock=clock,
            guid_factory=guid_factory,
            logger=logger)

    def test_can_initialize_trader(self):
        # Arrange
        # Act
        trader_id = self.trader.id

        # Assert
        self.assertEqual(TraderId('TESTER', '000'), trader_id)
        self.assertEqual(IdTag('000'), trader_id.order_id_tag)
        self.assertFalse(self.trader.is_running)
        self.assertEqual(0, len(self.trader.started_datetimes))
        self.assertEqual(0, len(self.trader.stopped_datetimes))
        self.assertEqual(2, len(self.trader.strategy_status()))

    def test_can_get_strategy_status(self):
        # Arrange
        # Act
        status = self.trader.strategy_status()

        # Assert
        self.assertTrue(StrategyId('EmptyStrategy', '001') in status)
        self.assertTrue(StrategyId('EmptyStrategy', '002') in status)
        self.assertFalse(status[StrategyId('EmptyStrategy', '001')])
        self.assertFalse(status[StrategyId('EmptyStrategy', '002')])
        self.assertEqual(2, len(status))

    def test_can_change_strategies(self):
        # Arrange
        strategies = [EmptyStrategy('003'),
                      EmptyStrategy('004')]

        # Act
        self.trader.initialize_strategies(strategies)

        # Assert
        self.assertTrue(strategies[0].id in self.trader.strategy_status())
        self.assertTrue(strategies[1].id in self.trader.strategy_status())
        self.assertEqual(2, len(self.trader.strategy_status()))

    def test_trader_detects_none_unique_identifiers(self):
        # Arrange
        strategies = [EmptyStrategy('000'),
                      EmptyStrategy('000')]

        # Act
        self.assertRaises(ValueError, self.trader.initialize_strategies, strategies)

    def test_can_start_a_trader(self):
        # Arrange
        # Act
        self.trader.start()

        # Assert
        self.assertTrue(self.trader.is_running)
        self.assertEqual(1, len(self.trader.started_datetimes))
        self.assertEqual(0, len(self.trader.stopped_datetimes))
        self.assertTrue(StrategyId('EmptyStrategy', '001') in self.trader.strategy_status())
        self.assertTrue(StrategyId('EmptyStrategy', '002') in self.trader.strategy_status())
        self.assertTrue(self.trader.strategy_status()[StrategyId('EmptyStrategy', '001')])
        self.assertTrue(self.trader.strategy_status()[StrategyId('EmptyStrategy', '002')])

    def test_can_stop_a_running_trader(self):
        # Arrange
        self.trader.start()

        # Act
        self.trader.stop()

        # Assert
        self.assertFalse(self.trader.is_running)
        self.assertEqual(1, len(self.trader.started_datetimes))
        self.assertEqual(1, len(self.trader.stopped_datetimes))
        self.assertTrue(StrategyId('EmptyStrategy', '001') in self.trader.strategy_status())
        self.assertTrue(StrategyId('EmptyStrategy', '002') in self.trader.strategy_status())
        self.assertFalse(self.trader.strategy_status()[StrategyId('EmptyStrategy', '001')])
        self.assertFalse(self.trader.strategy_status()[StrategyId('EmptyStrategy', '002')])
