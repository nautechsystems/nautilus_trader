#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_analyzer.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from datetime import datetime
from matplotlib import pyplot as plt

from inv_trader.common.clock import TestClock
from inv_trader.model.enums import Venue, OrderSide
from inv_trader.model.objects import ValidString, Quantity, Symbol, Price, Money
from inv_trader.model.order import OrderFactory
from inv_trader.model.events import OrderFilled
from inv_trader.model.identifiers import GUID, OrderId, PositionId, ExecutionId, ExecutionTicket
from inv_trader.model.position import Position
from inv_trader.portfolio.analyzer import Analyzer
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = Symbol('AUDUSD', Venue.FXCM)
GBPUSD_FXCM = Symbol('GBPUSD', Venue.FXCM)


class PortfolioTestsTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.analyzer = Analyzer()

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

    # def test_can_add_buy_transaction(self):
    #     # Arrange
    #     order_factory = OrderFactory(
    #         id_tag_trader=ValidString('001'),
    #         id_tag_strategy=ValidString('001'),
    #         clock=TestClock())
    #
    #     order = order_factory.market(
    #         AUDUSD_FXCM,
    #         OrderSide.BUY,
    #         Quantity(100000))
    #
    #     event = OrderFilled(
    #         order.symbol,
    #         order.id,
    #         ExecutionId('SOME_EXEC_ID_1'),
    #         ExecutionTicket('SOME_EXEC_TICKET_1'),
    #         order.side,
    #         order.quantity,
    #         Price('1.00001'),
    #         UNIX_EPOCH,
    #         GUID(uuid.uuid4()),
    #         UNIX_EPOCH)
    #
    #     # Act
    #     self.analyzer.add_transaction(event)
    #     result = self.analyzer.get_transactions()
    #
    #     # Assert
    #     self.assertEqual(1, len(result))
    #     self.assertEqual('AUDUSD.FXCM', result.iloc[0]['symbol'])
    #     self.assertEqual(100000, result.iloc[0]['amount'])
    #     self.assertEqual('1.00001', result.iloc[0]['price'])
    #
    # def test_can_add_sell_transaction(self):
    #     # Arrange
    #     order_factory = OrderFactory(
    #         id_tag_trader=ValidString('001'),
    #         id_tag_strategy=ValidString('001'),
    #         clock=TestClock())
    #
    #     order = order_factory.market(
    #         AUDUSD_FXCM,
    #         OrderSide.SELL,
    #         Quantity(100000))
    #
    #     event = OrderFilled(
    #         order.symbol,
    #         order.id,
    #         ExecutionId('SOME_EXEC_ID_1'),
    #         ExecutionTicket('SOME_EXEC_TICKET_1'),
    #         order.side,
    #         order.quantity,
    #         Price('1.00001'),
    #         UNIX_EPOCH,
    #         GUID(uuid.uuid4()),
    #         UNIX_EPOCH)
    #
    #     # Act
    #     self.analyzer.add_transaction(event)
    #     result = self.analyzer.get_transactions()
    #
    #     # Assert
    #     self.assertEqual(1, len(result))
    #     self.assertEqual('AUDUSD.FXCM', result.iloc[0]['symbol'])
    #     self.assertEqual(-100000, result.iloc[0]['amount'])
    #     self.assertEqual('1.00001', result.iloc[0]['price'])

    def test_can_add_positions(self):
        # Arrange
        position1 = Position(
            AUDUSD_FXCM,
            PositionId('1'),
            UNIX_EPOCH)

        position2 = Position(
            GBPUSD_FXCM,
            PositionId('1'),
            UNIX_EPOCH)

        positions = [position1, position2]

        # Act
        self.analyzer.add_positions(UNIX_EPOCH, positions, Money(100000))

        # Assert
        print(self.analyzer.get_positions())
