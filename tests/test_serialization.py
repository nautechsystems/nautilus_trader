#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_serialization.py" company="Invariance Pte">
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
from inv_trader.model.order import Order
from inv_trader.model.events import OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events import OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events import OrderPartiallyFilled, OrderFilled
from inv_trader.serialization import MsgPackEventSerializer
from inv_trader.serialization import MsgPackOrderSerializer
from inv_trader.serialization import MsgPackCommandSerializer
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = Symbol('AUDUSD', Venue.FXCM)
GBPUSD_FXCM = Symbol('GBPUSD', Venue.FXCM)


class MsgPackOrderSerializerTests(unittest.TestCase):

    def test_can_serialize_and_deserialize_market_orders(self):
        # Arrange
        serializer = MsgPackOrderSerializer()

        order = Order(
            AUDUSD_FXCM,
            'O123456',
            'SCALPER01_SL',
            OrderSide.BUY,
            OrderType.MARKET,
            100000,
            UNIX_EPOCH,
            price=None,
            time_in_force=TimeInForce.DAY,
            expire_time=None)

        # Act
        serialized = serializer.serialize(order)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(order, deserialized)

    def test_can_serialize_and_deserialize_limit_orders(self):
        # Arrange
        serializer = MsgPackOrderSerializer()

        order = Order(
            AUDUSD_FXCM,
            'O123456',
            'SCALPER01_SL',
            OrderSide.BUY,
            OrderType.STOP_LIMIT,
            100000,
            UNIX_EPOCH,
            Decimal('1.00000'),
            TimeInForce.DAY,
            expire_time=None)

        # Act
        serialized = serializer.serialize(order)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(order, deserialized)


class MsgPackEventSerializerTests(unittest.TestCase):

    def test_can_deserialize_order_submitted_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Hex bytes string from C# MsgPack.Cli
        hex_string = ('86a673796d626f6cab4155445553442e4658434da86f726465725f69'
                      '64ab537475624f726465724964a86576656e745f6964d92463636338'
                      '353066662d333062352d343366652d386535362d3465616136333932'
                      '37653531af6576656e745f74696d657374616d70b8313937302d3031'
                      '2d30315430303a30303a30302e3030305aaa6576656e745f74797065'
                      'af6f726465725f7375626d6974746564ae7375626d69747465645f74'
                      '696d65b8313937302d30312d30315430303a30303a30302e3030305a')

        body = bytearray.fromhex(hex_string)

        # Act
        result = serializer.deserialize(body)

        # Assert - Warning can be ignored (its because PyCharm doesn't know the type).
        self.assertTrue(isinstance(result, OrderSubmitted))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual('StubOrderId', result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.submitted_time)
        self.assertTrue(isinstance(result.event_id, UUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.event_timestamp)

    def test_can_deserialize_order_accepted_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Hex bytes string from C# MsgPack.Cli
        hex_string = ('86a673796d626f6cab4155445553442e4658434da86f726465725f69'
                      '64ab537475624f726465724964a86576656e745f6964d92466653334'
                      '306363622d633835362d343537352d383866622d3634626533303937'
                      '66623330af6576656e745f74696d657374616d70b8313937302d3031'
                      '2d30315430303a30303a30302e3030305aaa6576656e745f74797065'
                      'ae6f726465725f6163636570746564ad61636365707465645f74696d'
                      '65b8313937302d30312d30315430303a30303a30302e3030305a')

        body = bytearray.fromhex(hex_string)

        # Act
        result = serializer.deserialize(body)

        # Assert - Warnings can be ignored (its because PyCharm doesn't know the type).
        self.assertTrue(isinstance(result, OrderAccepted))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual('StubOrderId', result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.accepted_time)
        self.assertTrue(isinstance(result.event_id, UUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.event_timestamp)

    def test_can_deserialize_order_rejected_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Hex bytes string from C# MsgPack.Cli
        hex_string = ('87a673796d626f6cab4155445553442e4658434da86f726465725f69'
                      '64ab537475624f726465724964a86576656e745f6964d92463366638'
                      '313830392d373566662d343263342d396336632d6530313636313763'
                      '64393135af6576656e745f74696d657374616d70b8313937302d3031'
                      '2d30315430303a30303a30302e3030305aaa6576656e745f74797065'
                      'ae6f726465725f72656a6563746564ad72656a65637465645f74696d'
                      '65b8313937302d30312d30315430303a30303a30302e3030305aaf72'
                      '656a65637465645f726561736f6ead494e56414c49445f4f52444552')

        body = bytearray.fromhex(hex_string)

        # Act
        result = serializer.deserialize(body)

        # Assert - Warnings can be ignored (its because PyCharm doesn't know the type).
        self.assertTrue(isinstance(result, OrderRejected))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual('StubOrderId', result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.rejected_time)
        self.assertEqual('INVALID_ORDER', result.rejected_reason)
        self.assertTrue(isinstance(result.event_id, UUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.event_timestamp)

    def test_can_deserialize_order_working_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

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

        body = bytearray.fromhex(hex_string)

        # Act
        result = serializer.deserialize(body)

        # Assert - Warnings can be ignored (its because PyCharm doesn't know the type).
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

    def test_can_deserialize_order_working_events_with_expire_time(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Hex bytes string from C# MsgPack.Cli
        hex_string = ('8ea673796d626f6cab4155445553442e4658434da86f726465725f69'
                      '64ab537475624f726465724964a86576656e745f6964d92430343962'
                      '636466612d646337642d343665332d616665342d3461323338656638'
                      '37346632af6576656e745f74696d657374616d70b8313937302d3031'
                      '2d30315430303a30303a30302e3030305aaa6576656e745f74797065'
                      'ad6f726465725f776f726b696e67af6f726465725f69645f62726f6b'
                      '6572a742313233343536a56c6162656ca94f3132333435365f45aa6f'
                      '726465725f73696465a3425559aa6f726465725f74797065ab53544f'
                      '505f4d41524b4554a87175616e7469747901a57072696365a3312e30'
                      'ad74696d655f696e5f666f726365a3475444ab6578706972655f7469'
                      '6d65b8313937302d30312d30315430303a30313a30302e3030305aac'
                      '776f726b696e675f74696d65b8313937302d30312d30315430303a30'
                      '303a30302e3030305a')

        body = bytearray.fromhex(hex_string)

        # Act
        result = serializer.deserialize(body)
        print(type(result.expire_time))
        # Assert - Warnings can be ignored (its because PyCharm doesn't know the type).
        self.assertTrue(isinstance(result, OrderWorking))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual('StubOrderId', result.order_id)
        self.assertEqual('B123456', result.broker_order_id)
        self.assertEqual('O123456_E', result.label)
        self.assertEqual(OrderType.STOP_MARKET, result.order_type)
        self.assertEqual(1, result.quantity)
        self.assertEqual(Decimal('1'), result.price)
        self.assertEqual(TimeInForce.GTD, result.time_in_force)
        self.assertEqual(datetime(1970, 1, 1, 0, 0, 0, 0, pytz.UTC), result.working_time)
        self.assertTrue(isinstance(result.event_id, UUID))
        self.assertEqual(datetime(1970, 1, 1, 0, 0, 0, 0, pytz.UTC), result.event_timestamp)
        self.assertEqual(datetime(1970, 1, 1, 0, 1, 0, 0, pytz.UTC), result.expire_time)

    def test_can_deserialize_order_cancelled_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Hex bytes string from C# MsgPack.Cli
        hex_string = ('86a673796d626f6cab4155445553442e4658434da86f726465725f69'
                      '64ab537475624f726465724964a86576656e745f6964d92438343263'
                      '343730622d663464302d343861302d383638322d3734303837323739'
                      '36333334af6576656e745f74696d657374616d70b8313937302d3031'
                      '2d30315430303a30303a30302e3030305aaa6576656e745f74797065'
                      'af6f726465725f63616e63656c6c6564ae63616e63656c6c65645f74'
                      '696d65b8313937302d30312d30315430303a30303a30302e3030305a')

        body = bytearray.fromhex(hex_string)

        # Act
        result = serializer.deserialize(body)

        # Assert - Warning can be ignored (its because PyCharm doesn't know the type).
        self.assertTrue(isinstance(result, OrderCancelled))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual('StubOrderId', result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.cancelled_time)
        self.assertTrue(isinstance(result.event_id, UUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.event_timestamp)

    def test_can_deserialize_order_cancel_reject_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Hex bytes string from C# MsgPack.Cli
        hex_string = ('88a673796d626f6cab4155445553442e4658434da86f726465725f69'
                      '64ab537475624f726465724964a86576656e745f6964d92432663936'
                      '313135652d313632392d346634302d383861342d3932373466323836'
                      '34613331af6576656e745f74696d657374616d70b8313937302d3031'
                      '2d30315430303a30303a30302e3030305aaa6576656e745f74797065'
                      'b36f726465725f63616e63656c5f72656a656374ad72656a65637465'
                      '645f74696d65b8313937302d30312d30315430303a30303a30302e30'
                      '30305ab172656a65637465645f726573706f6e7365b052454a454354'
                      '5f524553504f4e53453faf72656a65637465645f726561736f6eaf4f'
                      '524445525f4e4f545f464f554e44')

        body = bytearray.fromhex(hex_string)

        # Act
        result = serializer.deserialize(body)

        # Assert - Warnings can be ignored (its because PyCharm doesn't know the type).
        self.assertTrue(isinstance(result, OrderCancelReject))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual('StubOrderId', result.order_id)
        self.assertEqual('REJECT_RESPONSE?', result.cancel_reject_response)
        self.assertEqual('ORDER_NOT_FOUND', result.cancel_reject_reason)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.cancel_reject_time)
        self.assertTrue(isinstance(result.event_id, UUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.event_timestamp)

    def test_can_deserialize_order_modified_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Hex bytes string from C# MsgPack.Cli
        hex_string = ('88a673796d626f6cab4155445553442e4658434da86f726465725f69'
                      '64ab537475624f726465724964a86576656e745f6964d92430393835'
                      '373533362d623133622d343137612d386134382d3166383134636237'
                      '35663033af6576656e745f74696d657374616d70b8313937302d3031'
                      '2d30315430303a30303a30302e3030305aaa6576656e745f74797065'
                      'ae6f726465725f6d6f646966696564af6f726465725f69645f62726f'
                      '6b6572a742313233343536ae6d6f6469666965645f7072696365a132'
                      'ad6d6f6469666965645f74696d65b8313937302d30312d3031543030'
                      '3a30303a30302e3030305a')

        body = bytearray.fromhex(hex_string)

        # Act
        result = serializer.deserialize(body)

        # Assert - Warnings can be ignored (its because PyCharm doesn't know the type).
        self.assertTrue(isinstance(result, OrderModified))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual('StubOrderId', result.order_id)
        self.assertEqual('B123456', result.broker_order_id)
        self.assertEqual(Decimal('2'), result.modified_price)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.modified_time)
        self.assertTrue(isinstance(result.event_id, UUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.event_timestamp)

    def test_can_deserialize_order_expired_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Hex bytes string from C# MsgPack.Cli
        hex_string = ('86a673796d626f6cab4155445553442e4658434da86f726465725f69'
                      '64ab537475624f726465724964a86576656e745f6964d92430343064'
                      '643239342d383337652d343138382d626130622d6238393864653364'
                      '63386230af6576656e745f74696d657374616d70b8313937302d3031'
                      '2d30315430303a30303a30302e3030305aaa6576656e745f74797065'
                      'ad6f726465725f65787069726564ac657870697265645f74696d65b8'
                      '313937302d30312d30315430303a30303a30302e3030305a')

        body = bytearray.fromhex(hex_string)

        # Act
        result = serializer.deserialize(body)

        # Assert - Warning can be ignored (is because PyCharm doesn't know the type).
        self.assertTrue(isinstance(result, OrderExpired))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual('StubOrderId', result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.expired_time)
        self.assertTrue(isinstance(result.event_id, UUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.event_timestamp)

    def test_can_deserialize_order_partially_filled_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Hex bytes string from C# MsgPack.Cli
        hex_string = ('8ca673796d626f6cab4155445553442e4658434da86f726465725f69'
                      '64ab537475624f726465724964a86576656e745f6964d92434336632'
                      '303830362d343732622d343232322d616465392d6465666566643164'
                      '62616166af6576656e745f74696d657374616d70b8313937302d3031'
                      '2d30315430303a30303a30302e3030305aaa6576656e745f74797065'
                      'b66f726465725f7061727469616c6c795f66696c6c6564ac65786563'
                      '7574696f6e5f6964a745313233343536b0657865637574696f6e5f74'
                      '69636b6574a750313233343536aa6f726465725f73696465a3425559'
                      'af66696c6c65645f7175616e74697479d20000c350af6c6561766573'
                      '5f7175616e74697479d20000c350ad617665726167655f7072696365'
                      'a3322e30ae657865637574696f6e5f74696d65b8313937302d30312d'
                      '30315430303a30303a30302e3030305a')

        body = bytearray.fromhex(hex_string)

        # Act
        result = serializer.deserialize(body)

        # Assert - Warnings can be ignored (its because PyCharm doesn't know the type).
        self.assertTrue(isinstance(result, OrderPartiallyFilled))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual('StubOrderId', result.order_id)
        self.assertEqual('E123456', result.execution_id)
        self.assertEqual('P123456', result.execution_ticket)
        self.assertEqual(OrderSide.BUY, result.order_side)
        self.assertEqual(50000, result.filled_quantity)
        self.assertEqual(50000, result.leaves_quantity)
        self.assertEqual(Decimal('2'), result.average_price)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.execution_time)
        self.assertTrue(isinstance(result.event_id, UUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.event_timestamp)

    def test_can_deserialize_order_filled_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Hex bytes string from C# MsgPack.Cli
        hex_string = ('8ba673796d626f6cab4155445553442e4658434da86f726465725f69'
                      '64ab537475624f726465724964a86576656e745f6964d92462346634'
                      '393234332d613361612d343462652d616262622d3937616435643263'
                      '61333361af6576656e745f74696d657374616d70b8313937302d3031'
                      '2d30315430303a30303a30302e3030305aaa6576656e745f74797065'
                      'ac6f726465725f66696c6c6564ac657865637574696f6e5f6964a745'
                      '313233343536b0657865637574696f6e5f7469636b6574a750313233'
                      '343536aa6f726465725f73696465a3425559af66696c6c65645f7175'
                      '616e74697479d2000186a0ad617665726167655f7072696365a3322e'
                      '30ae657865637574696f6e5f74696d65b8313937302d30312d303154'
                      '30303a30303a30302e3030305a')

        body = bytearray.fromhex(hex_string)

        # Act
        result = serializer.deserialize(body)

        # Assert - Warnings can be ignored (its because PyCharm doesn't know the type).
        self.assertTrue(isinstance(result, OrderFilled))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual('StubOrderId', result.order_id)
        self.assertEqual('E123456', result.execution_id)
        self.assertEqual('P123456', result.execution_ticket)
        self.assertEqual(OrderSide.BUY, result.order_side)
        self.assertEqual(100000, result.filled_quantity)
        self.assertEqual(Decimal('2'), result.average_price)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.execution_time)
        self.assertTrue(isinstance(result.event_id, UUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC), result.event_timestamp)
