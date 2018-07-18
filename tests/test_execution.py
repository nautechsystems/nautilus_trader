#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_execution.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import pytz

from datetime import datetime
from decimal import Decimal
from uuid import UUID

from inv_trader.model.enums import Venue, OrderSide, OrderType, OrderStatus, TimeInForce
from inv_trader.model.objects import Symbol, Resolution, QuoteType, BarType, Bar
from inv_trader.model.events import OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events import OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events import OrderPartiallyFilled, OrderFilled
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

    def test_can_receive_bars(self):
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

    def test_can_deserialize_order_submitted_events(self):
        # Arrange
        client = LiveExecClient()

        # Hex bytes string from C# MsgPack.Cli
        hex_string = ('86a673796d626f6cab4155445553442e4658434da86f726465725f69'
                      '64ab537475624f726465724964a86576656e745f6964d92463636338'
                      '353066662d333062352d343366652d386535362d3465616136333932'
                      '37653531af6576656e745f74696d657374616d70b8313937302d3031'
                      '2d30315430303a30303a30302e3030305aaa6576656e745f74797065'
                      'af6f726465725f7375626d6974746564ae7375626d69747465645f74'
                      '696d65b8313937302d30312d30315430303a30303a30302e3030305a')

        body = bytes.fromhex(hex_string)

        # Act
        result = client._deserialize_order_event(body)

        # Assert - Warning can be ignored (is because PyCharm doesn't know the type).
        self.assertTrue(isinstance(result, OrderSubmitted))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual('StubOrderId', result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.submitted_time)
        self.assertTrue(isinstance(result.event_id, UUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.event_timestamp)

    def test_can_parse_order_accepted_events(self):
        # Arrange
        client = LiveExecClient()

        # Hex bytes string from C# MsgPack.Cli
        hex_string = ('86a673796d626f6cab4155445553442e4658434da86f726465725f69'
                      '64ab537475624f726465724964a86576656e745f6964d92466653334'
                      '306363622d633835362d343537352d383866622d3634626533303937'
                      '66623330af6576656e745f74696d657374616d70b8313937302d3031'
                      '2d30315430303a30303a30302e3030305aaa6576656e745f74797065'
                      'ae6f726465725f6163636570746564ad61636365707465645f74696d'
                      '65b8313937302d30312d30315430303a30303a30302e3030305a')

        body = bytes.fromhex(hex_string)

        # Act
        result = client._deserialize_order_event(body)

        # Assert
        self.assertTrue(isinstance(result, OrderAccepted))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual('StubOrderId', result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.accepted_time)
        self.assertTrue(isinstance(result.event_id, UUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.event_timestamp)

    def test_can_parse_order_rejected_events(self):
        # Arrange
        client = LiveExecClient()

        # Hex bytes string from C# MsgPack.Cli
        hex_string = ('87a673796d626f6cab4155445553442e4658434da86f726465725f69'
                      '64ab537475624f726465724964a86576656e745f6964d92463366638'
                      '313830392d373566662d343263342d396336632d6530313636313763'
                      '64393135af6576656e745f74696d657374616d70b8313937302d3031'
                      '2d30315430303a30303a30302e3030305aaa6576656e745f74797065'
                      'ae6f726465725f72656a6563746564ad72656a65637465645f74696d'
                      '65b8313937302d30312d30315430303a30303a30302e3030305aaf72'
                      '656a65637465645f726561736f6ead494e56414c49445f4f52444552')

        body = bytes.fromhex(hex_string)

        # Act
        result = client._deserialize_order_event(body)

        # Assert
        self.assertTrue(isinstance(result, OrderRejected))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual('StubOrderId', result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.rejected_time)
        self.assertEqual('INVALID_ORDER', result.rejected_reason)
        self.assertTrue(isinstance(result.event_id, UUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.event_timestamp)

    def test_can_parse_order_working_events(self):
        # Arrange
        client = LiveExecClient()

        # Hex bytes string from C# MsgPack.Cli
        hex_string = ('8ea673796d626f6cab4155445553442e4658434da86f726465725f69'
                      '64ab537475624f726465724964a86576656e745f6964d92434353832'
                      '333066372d613138662d346464622d623566302d6233636436356538'
                      '31326537af6576656e745f74696d657374616d70b8313937302d3031'
                      '2d30315430303a30303a30302e3030305aaa6576656e745f74797065'
                      'ad6f726465725f776f726b696e67af6f726465725f69645f62726f6b'
                      '6572a742313233343536a56c6162656ca94f3132333435365f45aa6f'
                      '726465725f73696465a3425559aa6f726465725f74797065ab53544f'
                      '505f4d41524b4554a87175616e7469747901a57072696365a3312e30'
                      'ad74696d655f696e5f666f726365a3444159ab6578706972655f7469'
                      '6d65a46e6f6e65ac776f726b696e675f74696d65b8313937302d3031'
                      '2d30315430303a30303a30302e3030305a')

        body = bytes.fromhex(hex_string)

        # Act
        result = client._deserialize_order_event(body)

        # Assert
        self.assertTrue(isinstance(result, OrderWorking))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual('StubOrderId', result.order_id)
        self.assertEqual('B123456', result.broker_order_id)
        self.assertEqual('O123456_E', result.label)
        self.assertEqual(OrderType.STOP_MARKET, result.order_type)
        self.assertEqual(1, result.quantity)
        self.assertEqual(Decimal('1'), result.price)
        self.assertEqual(TimeInForce.DAY, result.time_in_force)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.working_time)
        self.assertTrue(isinstance(result.event_id, UUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.event_timestamp)
        self.assertIsNone(result.expire_time)

    def test_can_parse_order_cancelled_events(self):
        # Arrange
        client = LiveExecClient()
        event_string = 'order_cancelled:audusd.fxcm,O123456,1970-01-01T00:00:00.000Z'

        # Act
        result = client._deserialize_order_event(body)

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
        result = client._deserialize_order_event(body)

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
        result = client._deserialize_order_event(body)

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
        result = client._deserialize_order_event(body)

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
        result = client._deserialize_order_event(body)

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
        result = client._deserialize_order_event(body)

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
