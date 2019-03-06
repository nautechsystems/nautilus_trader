#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_analyzer.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import uuid

from datetime import date
from pyfolio.tears import create_full_tear_sheet
from pyfolio.tears import create_returns_tear_sheet

from inv_trader.common.clock import TestClock
from inv_trader.model.enums import Venue, OrderSide
from inv_trader.model.objects import ValidString, Quantity, Symbol, Price
from inv_trader.model.order import OrderFactory
from inv_trader.model.events import OrderFilled
from inv_trader.model.identifiers import GUID, OrderId, PositionId, ExecutionId, ExecutionTicket
from inv_trader.model.position import Position
from inv_trader.strategy import TradeStrategy
from inv_trader.portfolio.analyzer import Analyzer
from test_kit.mocks import MockExecClient
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = Symbol('audusd', Venue.FXCM)
GBPUSD_FXCM = Symbol('gbpusd', Venue.FXCM)


class PortfolioTestsTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.analyzer = Analyzer()

    def test_can_add_returns(self):
        # Arrange
        d1 = date(year=2010, month=1, day=1)
        d2 = date(year=2010, month=1, day=2)
        d3 = date(year=2010, month=1, day=3)
        d4 = date(year=2010, month=1, day=4)
        d5 = date(year=2010, month=1, day=5)
        d6 = date(year=2010, month=1, day=6)
        d7 = date(year=2010, month=1, day=7)
        d8 = date(year=2010, month=1, day=8)
        d9 = date(year=2010, month=1, day=9)
        d10 = date(year=2010, month=1, day=10)

        # Act
        self.analyzer.add_daily_returns(d1, 0.05)
        self.analyzer.add_daily_returns(d2, -0.10)
        self.analyzer.add_daily_returns(d3, 0.10)
        self.analyzer.add_daily_returns(d4, -0.21)
        self.analyzer.add_daily_returns(d5, 0.22)
        self.analyzer.add_daily_returns(d6, -0.23)
        self.analyzer.add_daily_returns(d7, 0.24)
        self.analyzer.add_daily_returns(d8, -0.25)
        self.analyzer.add_daily_returns(d9, 0.26)
        self.analyzer.add_daily_returns(d10, -0.10)
        result = self.analyzer.get_returns()

        # Assert
        print(result)
        #create_returns_tear_sheet(returns=result)
