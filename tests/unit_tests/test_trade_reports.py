# -------------------------------------------------------------------------------------------------
# <copyright file="test_trade_reports.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import uuid

from decimal import Decimal
from datetime import timedelta

from nautilus_trader.core.types import GUID
from nautilus_trader.common.clock import TestClock
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.objects import Quantity, Venue, Symbol, Price
from nautilus_trader.model.identifiers import ExecutionId, ExecutionTicket, PositionId
from nautilus_trader.model.order import OrderFactory
from nautilus_trader.model.position import Position
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.trade.reports import ReportProvider
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = Symbol('AUDUSD', Venue('FXCM'))
GBPUSD_FXCM = Symbol('GBPUSD', Venue('FXCM'))


class ReportProviderTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.order_factory = OrderFactory(
            id_tag_trader='001',
            id_tag_strategy='001',
            clock=TestClock())

    def test_can_produce_orders_report(self):
        # Arrange
        report_provider = ReportProvider()
        order1 = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(1500000),
            Price('0.80010'))

        order2 = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(1500000),
            Price('0.80000'))

        event = OrderFilled(
            order1.id,
            ExecutionId('SOME_EXEC_ID_1'),
            ExecutionTicket('SOME_EXEC_TICKET_1'),
            order1.symbol,
            order1.side,
            order1.quantity,
            Price('0.80011'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        order1.apply(event)

        orders = {order1.id: order1,
                  order2.id: order2}
        # Act
        report = report_provider.get_orders_report(orders)

        # Assert
        self.assertEqual(2, len(report))
        self.assertEqual('order_id', report.index.name)
        self.assertEqual(order1.id.value, report.index[0])
        self.assertEqual('AUDUSD', report.iloc[0]['symbol'])
        self.assertEqual('BUY', report.iloc[0]['side'])
        self.assertEqual('LIMIT', report.iloc[0]['type'])
        self.assertEqual(1500000, report.iloc[0]['quantity'])
        self.assertEqual(Decimal('0.80011'), report.iloc[0]['avg_price'])
        self.assertEqual(Decimal('0.00001'), report.iloc[0]['slippage'])

    def test_can_produce_order_fills_report(self):
        # Arrange
        report_provider = ReportProvider()
        order1 = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(1500000),
            Price('0.80010'))

        order2 = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(1500000),
            Price('0.80000'))

        event = OrderFilled(
            order1.id,
            ExecutionId('SOME_EXEC_ID_1'),
            ExecutionTicket('SOME_EXEC_TICKET_1'),
            order1.symbol,
            order1.side,
            order1.quantity,
            Price('0.80011'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        order1.apply(event)

        orders = {order1.id: order1,
                  order2.id: order2}
        # Act
        report = report_provider.get_order_fills_report(orders)

        # Assert
        self.assertEqual(1, len(report))
        self.assertEqual('order_id', report.index.name)
        self.assertEqual(order1.id.value, report.index[0])
        self.assertEqual('AUDUSD', report.iloc[0]['symbol'])
        self.assertEqual('BUY', report.iloc[0]['side'])
        self.assertEqual('LIMIT', report.iloc[0]['type'])
        self.assertEqual(1500000, report.iloc[0]['quantity'])
        self.assertEqual(Decimal('0.80011'), report.iloc[0]['avg_price'])
        self.assertEqual(Decimal('0.00001'), report.iloc[0]['slippage'])

    def test_can_produce_trades_report(self):
        # Arrange
        report_provider = ReportProvider()

        order1 = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(1000000))

        order2 = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(1000000))

        position1 = Position(
            order1.symbol,
            PositionId('P123456'),
            UNIX_EPOCH)

        position2 = Position(
            order1.symbol,
            PositionId('P234567'),
            UNIX_EPOCH)

        order_filled1 = OrderFilled(
            order1.id,
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            order1.symbol,
            order1.side,
            order1.quantity,
            Price('0.80000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        order_filled2 = OrderFilled(
            order2.id,
            ExecutionId('E123457'),
            ExecutionTicket('T123457'),
            order2.symbol,
            order2.side,
            order2.quantity,
            Price('0.80010'),
            UNIX_EPOCH + timedelta(minutes=5),
            GUID(uuid.uuid4()),
            UNIX_EPOCH + timedelta(minutes=5))

        position1.apply(order_filled1)
        position1.apply(order_filled2)

        positions = {position1.id: position1,
                     position2.id: position2}

        # Act
        report = report_provider.get_positions_report(positions)

        # Assert
        self.assertEqual(1, len(report))
        self.assertEqual('position_id', report.index.name)
        self.assertEqual(position1.id.value, report.index[0])
        self.assertEqual('AUDUSD', report.iloc[0]['symbol'])
        self.assertEqual('BUY', report.iloc[0]['direction'])
        self.assertEqual(1000000, report.iloc[0]['peak_quantity'])
        self.assertEqual(Decimal('0.80000'), report.iloc[0]['avg_entry_price'])
        self.assertEqual(Decimal('0.80010'), report.iloc[0]['avg_exit_price'])
        self.assertEqual(UNIX_EPOCH, report.iloc[0]['entry_time'])
        self.assertEqual(UNIX_EPOCH + timedelta(minutes=5), report.iloc[0]['exit_time'])
        self.assertEqual(Decimal('0.00010'), report.iloc[0]['points'])
        self.assertEqual(0.00012502074241638184, report.iloc[0]['return'])
