# -------------------------------------------------------------------------------------------------
# <copyright file="test_trade_performance.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from datetime import datetime

from nautilus_trader.model.objects import Venue, Symbol, Money
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.position import Position
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.trade.performance import PerformanceAnalyzer
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = Symbol('AUDUSD', Venue('FXCM'))
GBPUSD_FXCM = Symbol('GBPUSD', Venue('FXCM'))


class AnalyzerTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.analyzer = PerformanceAnalyzer()

    def test_can_add_returns(self):
        # Arrange
        t1 = datetime(year=2010, month=1, day=1)
        t2 = datetime(year=2010, month=1, day=2)
        t3 = datetime(year=2010, month=1, day=3)
        t4 = datetime(year=2010, month=1, day=4)
        t5 = datetime(year=2010, month=1, day=5)
        t6 = datetime(year=2010, month=1, day=6)
        t7 = datetime(year=2010, month=1, day=7)
        t8 = datetime(year=2010, month=1, day=8)
        t9 = datetime(year=2010, month=1, day=9)
        t10 = datetime(year=2010, month=1, day=10)

        # Act
        self.analyzer.add_return(t1, 0.05)
        self.analyzer.add_return(t2, -0.10)
        self.analyzer.add_return(t3, 0.10)
        self.analyzer.add_return(t4, -0.21)
        self.analyzer.add_return(t5, 0.22)
        self.analyzer.add_return(t6, -0.23)
        self.analyzer.add_return(t7, 0.24)
        self.analyzer.add_return(t8, -0.25)
        self.analyzer.add_return(t9, 0.26)
        self.analyzer.add_return(t10, -0.10)
        result = self.analyzer.get_returns()

        # Assert
        self.assertEqual(10, len(result))

    def test_can_add_transactions(self):
        # Arrange
        t1 = datetime(year=2010, month=1, day=1)
        t2 = datetime(year=2010, month=1, day=2)
        t3 = datetime(year=2010, month=1, day=3)

        # Act
        self.analyzer.add_transaction(t1, Money(1000000), Money(-100000))
        self.analyzer.add_transaction(t2, Money(900000), Money(-100000))
        self.analyzer.add_transaction(t3, Money(800000), Money(-100000))

        result = self.analyzer.get_equity_curve()

        # Assert
        self.assertEqual(3, len(result))

    def test_can_get_pnl_statistics(self):
        # Arrange
        t1 = datetime(year=2010, month=1, day=1)
        t2 = datetime(year=2010, month=1, day=2)
        t3 = datetime(year=2010, month=1, day=3)
        t4 = datetime(year=2010, month=1, day=4)
        t5 = datetime(year=2010, month=1, day=5)

        # Act
        self.analyzer.add_transaction(t1, Money(1000000), Money(-100000))
        self.analyzer.add_transaction(t2, Money(900000), Money(-50000))
        self.analyzer.add_transaction(t3, Money(850000), Money(-100000))
        self.analyzer.add_transaction(t4, Money(950000), Money(150000))
        self.analyzer.add_transaction(t5, Money(975000), Money(125000))

        # Assert
        self.assertEqual(Money(150000), self.analyzer.max_winner())
        self.assertEqual(Money(-100000), self.analyzer.max_loser())
        self.assertEqual(Money(125000), self.analyzer.min_winner())
        self.assertEqual(Money(-50000), self.analyzer.min_loser())
        self.assertEqual(Money(137500.00), self.analyzer.avg_winner())
        self.assertEqual(Money(-83333.33), self.analyzer.avg_loser())
        self.assertEqual(0.4000000059604645, self.analyzer.win_rate())
        self.assertEqual(5000.0009765625, self.analyzer.expectancy())

    def test_can_add_positions(self):
        # Arrange
        position1 = TestStubs.position()
        position2 = TestStubs.position()

        positions = [position1, position2]

        # Act
        self.analyzer.add_positions(UNIX_EPOCH, positions, Money(100000))

        # Assert
        print(self.analyzer.get_positions())
