# -------------------------------------------------------------------------------------------------
# <copyright file="test_common_execution.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import datetime
import time

from datetime import datetime, timezone, timedelta

from nautilus_trader.core.correctness import ConditionFailed
from nautilus_trader.common.clock import TestClock, LiveClock
from nautilus_trader.common.account import Account
from nautilus_trader.common.brokerage import CommissionCalculator
from nautilus_trader.common.portfolio import Portfolio
from nautilus_trader.common.guid import TestGuidFactory
from nautilus_trader.common.logger import TestLogger
from nautilus_trader.common.execution import InMemoryExecutionDatabase, ExecutionEngine
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.objects import Quantity, Venue, Symbol, Price, Money
from nautilus_trader.model.identifiers import TraderId, OrderId, PositionId
from nautilus_trader.model.position import Position
from nautilus_trader.model.enums import OrderStatus, Currency
from nautilus_trader.model.enums import MarketPosition
from nautilus_trader.model.objects import Tick, Bar
from nautilus_trader.model.events import TimeEvent
from nautilus_trader.model.identifiers import StrategyId, Label
from nautilus_trader.backtest.execution import BacktestExecClient
from nautilus_trader.backtest.models import FillModel

from nautilus_trader.trade.strategy import TradingStrategy
from test_kit.stubs import TestStubs
from test_kit.strategies import TestStrategy1

UNIX_EPOCH = TestStubs.unix_epoch()
USDJPY_FXCM = Symbol('USDJPY', Venue('FXCM'))
AUDUSD_FXCM = Symbol('AUDUSD', Venue('FXCM'))


class ExecutionEngineTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.clock = TestClock()
        self.guid_factory = TestGuidFactory()
        self.logger = TestLogger()
        self.account = Account()

        self.portfolio = Portfolio(
            clock=self.clock,
            guid_factory=self.guid_factory,
            logger=self.logger)

        self.exec_db = InMemoryExecutionDatabase(trader_id=TraderId('000'), logger=self.logger)
        self.exec_engine = ExecutionEngine(
            database=self.exec_db,
            account=self.account,
            portfolio=self.portfolio,
            clock=self.clock,
            guid_factory=self.guid_factory)

    def test_can_initialize(self):
        # Arrange
        strategy = TradingStrategy(id_tag_strategy='001')

        # Act
        result = strategy.__hash__()

        # Assert
        # If this passes then result must be an int.
        self.assertTrue(result != 0)
