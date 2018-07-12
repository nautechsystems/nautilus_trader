#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_execution.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import uuid
import pytz

from datetime import datetime
from decimal import Decimal

from inv_trader.model.enums import Venue, OrderSide, OrderType, OrderStatus, TimeInForce
from inv_trader.model.objects import Symbol, Resolution, QuoteType, BarType, Bar
from inv_trader.model.events import OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events import OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events import OrderPartiallyFilled, OrderFilled
from inv_trader.factories import OrderFactory
from inv_trader.execution import ExecutionClient, LiveExecClient
from test_kit.stubs import TestStubs
from test_kit.mocks import MockExecClient
from test_kit.objects import ObjectStorer
from test_kit.strategies import TestStrategy1

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = Symbol('AUDUSD', Venue.FXCM)
GBPUSD_FXCM = Symbol('GBPUSD', Venue.FXCM)


class ExecutionClientTests(unittest.TestCase):

    def test_can_register_strategy(self):
        # Arrange
        storer = ObjectStorer()
        strategy = TestStrategy1(storer)
        exec_client = ExecutionClient()
        exec_client.register_strategy(strategy)

        # Act
        result = strategy._exec_client

        # Assert
        self.assertEqual(exec_client, result)

    def test_can_receive_events(self):
        # Arrange
        storer = ObjectStorer()
        strategy = TestStrategy1(storer)
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        strategy.start()

        bar_type = BarType(GBPUSD_FXCM,
                           1,
                           Resolution.SECOND,
                           QuoteType.MID)

        bar1 = Bar(
            Decimal('1.00001'),
            Decimal('1.00004'),
            Decimal('1.00003'),
            Decimal('1.00002'),
            100000,
            datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC))

        bar2 = Bar(
            Decimal('1.00011'),
            Decimal('1.00014'),
            Decimal('1.00013'),
            Decimal('1.00012'),
            100000,
            datetime(1970, 1, 1, 00, 00, 1, 0, pytz.UTC))

        # Act
        strategy._update_bars(bar_type, bar1)
        strategy._update_bars(bar_type, bar2)
        result = storer.get_store[-1]

        # Assert
        self.assertTrue(isinstance(result, OrderWorking))


class LiveExecClientTests(unittest.TestCase):

    def test_can_parse_order_submitted_events(self):
        # Arrange
        client = LiveExecClient()
        event_string = 'order_submitted:audusd.fxcm,O123456,1970-01-01T00:00:00.000Z'

        # Act
        result = client._parse_order_event(event_string)

        # Assert
        self.assertTrue(isinstance(result, OrderSubmitted))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual('O123456', result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.submitted_time)

    def test_can_parse_order_accepted_events(self):
        # Arrange
        client = LiveExecClient()
        event_string = 'order_accepted:audusd.fxcm,O123456,1970-01-01T00:00:00.000Z'

        # Act
        result = client._parse_order_event(event_string)

        # Assert
        self.assertTrue(isinstance(result, OrderAccepted))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual('O123456', result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.accepted_time)

    def test_can_parse_order_rejected_events(self):
        # Arrange
        client = LiveExecClient()
        event_string = 'order_rejected:audusd.fxcm,O123456,1970-01-01T00:00:00.000Z,INVALID_ORDER_ID'

        # Act
        result = client._parse_order_event(event_string)

        # Assert
        self.assertTrue(isinstance(result, OrderRejected))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual('O123456', result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.rejected_time)
        self.assertEqual('INVALID_ORDER_ID', result.rejected_reason)

    def test_can_parse_order_working_events(self):
        # Arrange
        client = LiveExecClient()
        event_string = 'order_working:audusd.fxcm,O123456,B123456,1970-01-01T00:00:00.000Z'

        # Act
        result = client._parse_order_event(event_string)

        # Assert
        self.assertTrue(isinstance(result, OrderWorking))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual('O123456', result.order_id)
        self.assertEqual('B123456', result.broker_order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.working_time)

    def test_can_parse_order_cancelled_events(self):
        # Arrange
        client = LiveExecClient()
        event_string = 'order_cancelled:audusd.fxcm,O123456,1970-01-01T00:00:00.000Z'

        # Act
        result = client._parse_order_event(event_string)

        # Assert
        self.assertTrue(isinstance(result, OrderCancelled))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual('O123456', result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.cancelled_time)

    def test_can_parse_order_cancel_reject_events(self):
        # Arrange
        client = LiveExecClient()
        event_string = 'order_cancel_reject:audusd.fxcm,O123456,1970-01-01T00:00:00.000Z,ORDER_DOES_NOT_EXIST'

        # Act
        result = client._parse_order_event(event_string)

        # Assert
        self.assertTrue(isinstance(result, OrderCancelReject))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual('O123456', result.order_id)
        self.assertEqual('ORDER_DOES_NOT_EXIST', result.cancel_reject_reason)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.cancel_reject_time)

    def test_can_parse_order_modified_events(self):
        # Arrange
        client = LiveExecClient()
        event_string = 'order_modified:audusd.fxcm,O123456,B123456,1.00001,1970-01-01T00:00:00.000Z'

        # Act
        result = client._parse_order_event(event_string)

        # Assert
        self.assertTrue(isinstance(result, OrderModified))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual('O123456', result.order_id)
        self.assertEqual('B123456', result.broker_order_id)
        self.assertEqual(Decimal('1.00001'), result.modified_price)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.modified_time)

    def test_can_parse_order_expired_events(self):
        # Arrange
        client = LiveExecClient()
        event_string = 'order_expired:audusd.fxcm,O123456,1970-01-01T00:00:00.000Z'

        # Act
        result = client._parse_order_event(event_string)

        # Assert
        self.assertTrue(isinstance(result, OrderExpired))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual('O123456', result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.expired_time)

    def test_can_parse_order_filled_events(self):
        # Arrange
        client = LiveExecClient()
        event_string = 'order_filled:audusd.fxcm,O123456,EX123456,P123456,BUY,100000,1.50001,1970-01-01T00:00:00.000Z'

        # Act
        result = client._parse_order_event(event_string)

        # Assert
        self.assertTrue(isinstance(result, OrderFilled))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual('O123456', result.order_id)
        self.assertEqual('EX123456', result.execution_id)
        self.assertEqual('P123456', result.execution_ticket)
        self.assertEqual(OrderSide.BUY, result.order_side)
        self.assertEqual(100000, result.filled_quantity)
        self.assertEqual(Decimal('1.50001'), result.average_price)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.execution_time)

    def test_can_parse_order_partially_filled_events(self):
        # Arrange
        client = LiveExecClient()
        event_string = 'order_partially_filled:audusd.fxcm,O123456,EX123456,P123456,BUY,50000,50000,1.50001,1970-01-01T00:00:00.000Z'

        # Act
        result = client._parse_order_event(event_string)

        # Assert
        self.assertTrue(isinstance(result, OrderPartiallyFilled))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual('O123456', result.order_id)
        self.assertEqual('EX123456', result.execution_id)
        self.assertEqual('P123456', result.execution_ticket)
        self.assertEqual(OrderSide.BUY, result.order_side)
        self.assertEqual(50000, result.filled_quantity)
        self.assertEqual(50000, result.leaves_quantity)
        self.assertEqual(Decimal('1.50001'), result.average_price)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.execution_time)
