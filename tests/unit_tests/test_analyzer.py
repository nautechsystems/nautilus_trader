#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_analyzer.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

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
from test_kit.data import TestDataProvider

AUDUSD_FXCM = Symbol('audusd', Venue.FXCM)


class PortfolioTestsTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.analyzer = Analyzer()

    def test_pyfolio_output(self):
        # Arrange
        returns = TestDataProvider.test_returns()
        positions = TestDataProvider.test_positions()
        #transactions = TestDataProvider.test_transactions()

        # Act
        # Assert
        # create_full_tear_sheet(returns,
        #                        positions=positions,
        #                        transactions=transactions,
        #                        benchmark_rets=returns,
        #                        slippage=1,
        #                        live_start_date=returns.index[-20],
        #                        round_trips=True,
        #                        hide_positions=True,
        #                        cone_std=1,
        #                        bootstrap=True)

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
        self.analyzer.add_return(d1, 0.05)
        self.analyzer.add_return(d2, -0.10)
        self.analyzer.add_return(d3, 0.10)
        self.analyzer.add_return(d4, -0.21)
        self.analyzer.add_return(d5, 0.22)
        self.analyzer.add_return(d6, -0.23)
        self.analyzer.add_return(d7, 0.24)
        self.analyzer.add_return(d8, -0.25)
        self.analyzer.add_return(d9, 0.26)
        self.analyzer.add_return(d10, -0.10)
        result = self.analyzer.get_returns()

        # Assert
        print(result)
        create_returns_tear_sheet(returns=result)
