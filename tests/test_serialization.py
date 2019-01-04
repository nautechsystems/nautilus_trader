#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_serialization.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import uuid

from datetime import datetime, timezone

from inv_trader.core.decimal import Decimal
from inv_trader.common.serialization import InstrumentSerializer
from inv_trader.commands import SubmitOrder, CancelOrder, ModifyOrder
from inv_trader.commands import CollateralInquiry
from inv_trader.model.enums import Venue, OrderSide, OrderType, TimeInForce
from inv_trader.model.enums import CurrencyCode, SecurityType
from inv_trader.model.identifiers import GUID, Label, OrderId, ExecutionId, ExecutionTicket
from inv_trader.model.objects import Symbol, Price, Instrument
from inv_trader.model.order import Order, OrderFactory
from inv_trader.model.events import OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events import OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events import OrderPartiallyFilled, OrderFilled, AccountEvent
from inv_trader.serialization import MsgPackOrderSerializer
from inv_trader.serialization import MsgPackCommandSerializer
from inv_trader.serialization import MsgPackEventSerializer
from inv_trader.serialization import _convert_price_to_string, _convert_datetime_to_string
from inv_trader.serialization import _convert_string_to_price, _convert_string_to_datetime
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = Symbol('AUDUSD', Venue.FXCM)
GBPUSD_FXCM = Symbol('GBPUSD', Venue.FXCM)


class SerializationFunctionTests(unittest.TestCase):

    def test_can_convert_price_to_string_from_none(self):
        # Arrange
        # Act
        result = _convert_price_to_string(None)

        # Assert
        self.assertEqual('NONE', result)

    def test_can_convert_price_to_string_from_decimal(self):
        # Arrange
        # Act
        result = _convert_price_to_string(Decimal('1.00000'))

        # Assert
        self.assertEqual('1.00000', result)

    def test_can_convert_string_to_price_from_none(self):
        # Arrange
        # Act
        result = _convert_string_to_price('NONE')

        # Assert
        self.assertEqual(None, result)

    def test_can_convert_string_to_price_from_decimal(self):
        # Arrange
        # Act
        result = _convert_string_to_price('1.00000')

        # Assert
        self.assertEqual(Decimal('1.00000'), result)

    def test_can_convert_expire_time_to_string_from_none(self):
        # Arrange
        # Act
        result = _convert_datetime_to_string(None)

        # Assert
        self.assertEqual('NONE', result)

    def test_can_convert_expire_time_to_string_from_datetime(self):
        # Arrange
        # Act
        result = _convert_datetime_to_string(UNIX_EPOCH)

        # Assert
        self.assertEqual('1970-01-01T00:00:00.000Z', result)

    def test_can_convert_string_to_expire_time_from_datetime(self):
        # Arrange
        # Act
        result = _convert_string_to_datetime('1970-01-01T00:00:00.000Z')

        # Assert
        self.assertEqual(UNIX_EPOCH, result)

    def test_can_convert_string_to_expire_time_from_none(self):
        # Arrange
        # Act
        result = _convert_string_to_datetime('NONE')

        # Assert
        self.assertEqual(None, result)


class MsgPackOrderSerializerTests(unittest.TestCase):

    def test_can_serialize_and_deserialize_market_orders(self):
        # Arrange
        serializer = MsgPackOrderSerializer()

        order = OrderFactory.market(
            AUDUSD_FXCM,
            OrderId('O123456'),
            Label('SCALPER01_SL'),
            OrderSide.BUY,
            100000)

        # Act
        serialized = serializer.serialize(order)
        deserialized = serializer.deserialize(serialized)

        # Assert
        print(serialized.hex())
        self.assertEqual(order, deserialized)

    def test_can_serialize_and_deserialize_limit_orders(self):
        # Arrange
        serializer = MsgPackOrderSerializer()

        order = OrderFactory.limit(
            AUDUSD_FXCM,
            OrderId('O123456'),
            Label('S1_SL'),
            OrderSide.BUY,
            100000,
            Price.create(1.00000, 5),
            TimeInForce.DAY)

        # Act
        serialized = serializer.serialize(order)
        deserialized = serializer.deserialize(serialized)

        # Assert
        print(serialized.hex())
        self.assertEqual(order, deserialized)

    def test_can_serialize_and_deserialize_limit_orders_with_expire_time(self):
        # Arrange
        serializer = MsgPackOrderSerializer()

        order = Order(
            AUDUSD_FXCM,
            OrderId('O123456'),
            Label('S1_SL'),
            OrderSide.BUY,
            OrderType.LIMIT,
            100000,
            UNIX_EPOCH,
            Decimal('1.00000'),
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(order)
        deserialized = serializer.deserialize(serialized)

        # Assert
        print(serialized.hex())
        self.assertEqual(order, deserialized)

    def test_can_serialize_and_deserialize_stop_limit_orders(self):
        # Arrange
        serializer = MsgPackOrderSerializer()

        order = Order(
            AUDUSD_FXCM,
            OrderId('O123456'),
            Label('S1_SL'),
            OrderSide.BUY,
            OrderType.STOP_LIMIT,
            100000,
            UNIX_EPOCH,
            Decimal('1.00000'))

        # Act
        serialized = serializer.serialize(order)
        deserialized = serializer.deserialize(serialized)

        # Assert
        print(serialized.hex())
        self.assertEqual(order, deserialized)

    def test_can_serialize_and_deserialize_stop_limit_orders_with_expire_time(self):
        # Arrange
        serializer = MsgPackOrderSerializer()

        order = Order(
            AUDUSD_FXCM,
            OrderId('O123456'),
            Label('S1_SL'),
            OrderSide.BUY,
            OrderType.STOP_LIMIT,
            100000,
            UNIX_EPOCH,
            Decimal('1.00000'),
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(order)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(order, deserialized)


class MsgPackCommandSerializerTests(unittest.TestCase):

    def test_can_serialize_and_deserialize_submit_order_commands(self):
        # Arrange
        serializer = MsgPackCommandSerializer()

        order = OrderFactory.market(
            AUDUSD_FXCM,
            OrderId('O123456'),
            Label('S1_SL'),
            OrderSide.BUY,
            100000)

        command = SubmitOrder(order,
                              GUID(uuid.uuid4()),
                              UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(command)
        deserialized = serializer.deserialize(serialized)

        # Assert - ignore warning (PyCharm doesn't know the type).
        self.assertEqual(command, deserialized)
        self.assertEqual(order, deserialized.order)
        print(serialized.hex())
        print(command)

    def test_can_serialize_and_deserialize_cancel_order_commands(self):
        # Arrange
        serializer = MsgPackCommandSerializer()

        order = Order(
            AUDUSD_FXCM,
            OrderId('O123456'),
            Label('S1_SL'),
            OrderSide.BUY,
            OrderType.LIMIT,
            100000,
            UNIX_EPOCH,
            Decimal('1.00000'),
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH)

        command = CancelOrder(order,
                              'EXPIRED',
                              GUID(uuid.uuid4()),
                              UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(command)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(command, deserialized)
        self.assertEqual(order, deserialized.order)
        print(serialized.hex())

    def test_can_serialize_and_deserialize_modify_order_commands(self):
        # Arrange
        serializer = MsgPackCommandSerializer()

        order = Order(
            AUDUSD_FXCM,
            OrderId('O123456'),
            Label('S1_SL'),
            OrderSide.BUY,
            OrderType.LIMIT,
            100000,
            UNIX_EPOCH,
            Decimal('1.00000'),
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH)

        command = ModifyOrder(order,
                              Decimal('1.00001'),
                              GUID(uuid.uuid4()),
                              UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(command)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(command, deserialized)
        self.assertEqual(order, deserialized.order)
        print(serialized.hex())

    def test_can_serialized_and_deserialize_collateral_inquiry_requests(self):
        # Arrange
        serializer = MsgPackCommandSerializer()

        request = CollateralInquiry(GUID(uuid.uuid4()), UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(request)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, request)
        print(serialized.hex())


class MsgPackEventSerializerTests(unittest.TestCase):

    def test_can_serialized_and_deserialize_order_submitted_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        event = OrderSubmitted(AUDUSD_FXCM,
                               OrderId('O123456'),
                               UNIX_EPOCH,
                               GUID(uuid.uuid4()),
                               UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(event)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialized_and_deserialize_order_accepted_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        event = OrderAccepted(AUDUSD_FXCM,
                              OrderId('O123456'),
                              UNIX_EPOCH,
                              GUID(uuid.uuid4()),
                              UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(event)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialized_and_deserialize_order_rejected_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        event = OrderRejected(AUDUSD_FXCM,
                              OrderId('O123456'),
                              UNIX_EPOCH,
                              'ORDER_ID_INVALID',
                              GUID(uuid.uuid4()),
                              UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(event)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialized_and_deserialize_order_working_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        event = OrderWorking(
            AUDUSD_FXCM,
            OrderId('O123456'),
            OrderId('B123456'),
            Label('S1_PT'),
            OrderSide.SELL,
            OrderType.STOP_LIMIT,
            100000,
            Decimal('1.50000'),
            TimeInForce.DAY,
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH,
            expire_time=None)

        # Act
        serialized = serializer.serialize(event)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialized_and_deserialize_order_working_events_with_expire_time(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        event = OrderWorking(
            AUDUSD_FXCM,
            OrderId('O123456'),
            OrderId('B123456'),
            Label('S1_PT'),
            OrderSide.SELL,
            OrderType.STOP_LIMIT,
            100000,
            Decimal('1.50000'),
            TimeInForce.DAY,
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH,
            expire_time=UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(event)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialized_and_deserialize_order_cancelled_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        event = OrderCancelled(
            AUDUSD_FXCM,
            OrderId('O123456'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(event)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialized_and_deserialize_order_cancel_reject_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        event = OrderCancelReject(
            AUDUSD_FXCM,
            OrderId('O123456'),
            UNIX_EPOCH,
            'RESPONSE',
            'ORDER_DOES_NOT_EXIST',
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(event)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialized_and_deserialize_order_modified_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        event = OrderModified(
            AUDUSD_FXCM,
            OrderId('O123456'),
            OrderId('B123456'),
            Decimal('0.80010'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(event)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialized_and_deserialize_order_expired_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        event = OrderExpired(
            AUDUSD_FXCM,
            OrderId('O123456'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(event)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialized_and_deserialize_order_partially_filled_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        event = OrderPartiallyFilled(
            AUDUSD_FXCM,
            OrderId('O123456'),
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            OrderSide.SELL,
            50000,
            50000,
            Decimal('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(event)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialized_and_deserialize_order_filled_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        event = OrderFilled(
            AUDUSD_FXCM,
            OrderId('O123456'),
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            OrderSide.SELL,
            100000,
            Decimal('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(event)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_deserialize_order_submitted_events_from_csharp(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Hex bytes string from C# MsgPack.Cli
        hex_string = ('87aa6576656e745f74797065ab6f726465725f6576656e74a673796d'
                      '626f6cab4155445553442e4658434da86f726465725f6964ab537475'
                      '624f726465724964a86576656e745f6964d92438663132306439632d'
                      '396437362d343533362d613864302d376461656465623166343235af'
                      '6576656e745f74696d657374616d70b8313937302d30312d30315430'
                      '303a30303a30302e3030305aab6f726465725f6576656e74af6f7264'
                      '65725f7375626d6974746564ae7375626d69747465645f74696d65b8'
                      '313937302d30312d30315430303a30303a30302e3030305a')

        body = bytes.fromhex(hex_string)

        # Act
        result = serializer.deserialize(body)

        # Assert - Warning can be ignored (PyCharm doesn't know the type).
        self.assertTrue(isinstance(result, OrderSubmitted))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual(OrderId('StubOrderId'), result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.submitted_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_accepted_events_from_csharp(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Hex bytes string from C# MsgPack.Cli
        hex_string = ('87aa6576656e745f74797065ab6f726465725f6576656e74a673796d'
                      '626f6cab4155445553442e4658434da86f726465725f6964ab537475'
                      '624f726465724964a86576656e745f6964d92437653064363137342d'
                      '643233622d343334382d386436362d626231393766633637393765af'
                      '6576656e745f74696d657374616d70b8313937302d30312d30315430'
                      '303a30303a30302e3030305aab6f726465725f6576656e74ae6f7264'
                      '65725f6163636570746564ad61636365707465645f74696d65b83139'
                      '37302d30312d30315430303a30303a30302e3030305a')

        body = bytes.fromhex(hex_string)

        # Act
        result = serializer.deserialize(body)

        # Assert - Warnings can be ignored (PyCharm doesn't know the type).
        self.assertTrue(isinstance(result, OrderAccepted))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual(OrderId('StubOrderId'), result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.accepted_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_rejected_events_from_csharp(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Hex bytes string from C# MsgPack.Cli
        hex_string = ('88aa6576656e745f74797065ab6f726465725f6576656e74a673796d'
                      '626f6cab4155445553442e4658434da86f726465725f6964ab537475'
                      '624f726465724964a86576656e745f6964d92463343466373131342d'
                      '326432342d343635372d616635392d633634646538316461393639af'
                      '6576656e745f74696d657374616d70b8313937302d30312d30315430'
                      '303a30303a30302e3030305aab6f726465725f6576656e74ae6f7264'
                      '65725f72656a6563746564ad72656a65637465645f74696d65b83139'
                      '37302d30312d30315430303a30303a30302e3030305aaf72656a6563'
                      '7465645f726561736f6ead494e56414c49445f4f52444552')

        body = bytes.fromhex(hex_string)

        # Act
        result = serializer.deserialize(body)

        # Assert - Warnings can be ignored (PyCharm doesn't know the type).
        self.assertTrue(isinstance(result, OrderRejected))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual(OrderId('StubOrderId'), result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.rejected_time)
        self.assertEqual('INVALID_ORDER', result.rejected_reason)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_working_events_from_csharp(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Hex bytes string from C# MsgPack.Cli
        hex_string = ('8faa6576656e745f74797065ab6f726465725f6576656e74a673796d'
                      '626f6cab4155445553442e4658434da86f726465725f6964ab537475'
                      '624f726465724964a86576656e745f6964d92463653661363030652d'
                      '636166342d346366622d616539302d353061386433303166653037af'
                      '6576656e745f74696d657374616d70b8313937302d30312d30315430'
                      '303a30303a30302e3030305aab6f726465725f6576656e74ad6f7264'
                      '65725f776f726b696e67af6f726465725f69645f62726f6b6572a742'
                      '313233343536a56c6162656ca94f3132333435365f45aa6f72646572'
                      '5f73696465a3425559aa6f726465725f74797065ab53544f505f4d41'
                      '524b4554a87175616e7469747901a57072696365a3312e30ad74696d'
                      '655f696e5f666f726365a3444159ab6578706972655f74696d65a44e'
                      '4f4e45ac776f726b696e675f74696d65b8313937302d30312d303154'
                      '30303a30303a30302e3030305a')

        body = bytes.fromhex(hex_string)

        # Act
        result = serializer.deserialize(body)

        # Assert - Warnings can be ignored (PyCharm doesn't know the type).
        self.assertTrue(isinstance(result, OrderWorking))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual(OrderId('StubOrderId'), result.order_id)
        self.assertEqual(OrderId('B123456'), result.broker_order_id)
        self.assertEqual(Label('O123456_E'), result.label)
        self.assertEqual(OrderType.STOP_MARKET, result.order_type)
        self.assertEqual(1, result.quantity)
        self.assertEqual(Decimal('1'), result.price)
        self.assertEqual(TimeInForce.DAY, result.time_in_force)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.working_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)
        self.assertIsNone(result.expire_time)

    def test_can_deserialize_order_working_events_with_expire_time_from_csharp(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Hex bytes string from C# MsgPack.Cli
        hex_string = ('8faa6576656e745f74797065ab6f726465725f6576656e74a673796d'
                      '626f6cab4155445553442e4658434da86f726465725f6964ab537475'
                      '624f726465724964a86576656e745f6964d92463333436356364622d'
                      '626461382d346536662d623332392d326631613232393933396130af'
                      '6576656e745f74696d657374616d70b8313937302d30312d30315430'
                      '303a30303a30302e3030305aab6f726465725f6576656e74ad6f7264'
                      '65725f776f726b696e67af6f726465725f69645f62726f6b6572a742'
                      '313233343536a56c6162656ca94f3132333435365f45aa6f72646572'
                      '5f73696465a3425559aa6f726465725f74797065ab53544f505f4d41'
                      '524b4554a87175616e7469747901a57072696365a3312e30ad74696d'
                      '655f696e5f666f726365a3475444ab6578706972655f74696d65b831'
                      '3937302d30312d30315430303a30313a30302e3030305aac776f726b'
                      '696e675f74696d65b8313937302d30312d30315430303a30303a3030'
                      '2e3030305a')

        body = bytes.fromhex(hex_string)

        # Act
        result = serializer.deserialize(body)

        # Assert - Warnings can be ignored (PyCharm doesn't know the type).
        self.assertTrue(isinstance(result, OrderWorking))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual(OrderId('StubOrderId'), result.order_id)
        self.assertEqual(OrderId('B123456'), result.broker_order_id)
        self.assertEqual(Label('O123456_E'), result.label)
        self.assertEqual(OrderType.STOP_MARKET, result.order_type)
        self.assertEqual(1, result.quantity)
        self.assertEqual(Decimal('1'), result.price)
        self.assertEqual(TimeInForce.GTD, result.time_in_force)
        self.assertEqual(datetime(1970, 1, 1, 0, 0, 0, 0, timezone.utc), result.working_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 0, 0, 0, 0, timezone.utc), result.timestamp)
        self.assertEqual(datetime(1970, 1, 1, 0, 1, 0, 0, timezone.utc), result.expire_time)

    def test_can_deserialize_order_cancelled_events_from_csharp(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Hex bytes string from C# MsgPack.Cli
        hex_string = ('87aa6576656e745f74797065ab6f726465725f6576656e74a673796d'
                      '626f6cab4155445553442e4658434da86f726465725f6964ab537475'
                      '624f726465724964a86576656e745f6964d92437333766616632382d'
                      '386330352d346230642d396361332d373637396632313738393433af'
                      '6576656e745f74696d657374616d70b8313937302d30312d30315430'
                      '303a30303a30302e3030305aab6f726465725f6576656e74af6f7264'
                      '65725f63616e63656c6c6564ae63616e63656c6c65645f74696d65b8'
                      '313937302d30312d30315430303a30303a30302e3030305a')

        body = bytes.fromhex(hex_string)

        # Act
        result = serializer.deserialize(body)

        # Assert - Warning can be ignored (PyCharm doesn't know the type).
        self.assertTrue(isinstance(result, OrderCancelled))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual(OrderId('StubOrderId'), result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.cancelled_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_cancel_reject_events_from_csharp(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Hex bytes string from C# MsgPack.Cli
        hex_string = ('89aa6576656e745f74797065ab6f726465725f6576656e74a673796d'
                      '626f6cab4155445553442e4658434da86f726465725f6964ab537475'
                      '624f726465724964a86576656e745f6964d92437366432353535372d'
                      '363534312d346138652d383534302d646663373534613935626164af'
                      '6576656e745f74696d657374616d70b8313937302d30312d30315430'
                      '303a30303a30302e3030305aab6f726465725f6576656e74b36f7264'
                      '65725f63616e63656c5f72656a656374ad72656a65637465645f7469'
                      '6d65b8313937302d30312d30315430303a30303a30302e3030305ab1'
                      '72656a65637465645f726573706f6e7365b052454a4543545f524553'
                      '504f4e53453faf72656a65637465645f726561736f6eaf4f52444552'
                      '5f4e4f545f464f554e44')

        body = bytes.fromhex(hex_string)

        # Act
        result = serializer.deserialize(body)

        # Assert - Warnings can be ignored (PyCharm doesn't know the type).
        self.assertTrue(isinstance(result, OrderCancelReject))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual(OrderId('StubOrderId'), result.order_id)
        self.assertEqual('REJECT_RESPONSE?', result.cancel_reject_response)
        self.assertEqual('ORDER_NOT_FOUND', result.cancel_reject_reason)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.cancel_reject_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_modified_events_from_csharp(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Hex bytes string from C# MsgPack.Cli
        hex_string = ('89aa6576656e745f74797065ab6f726465725f6576656e74a673796d'
                      '626f6cab4155445553442e4658434da86f726465725f6964ab537475'
                      '624f726465724964a86576656e745f6964d92430313461333662302d'
                      '316531612d343434642d393830332d626539656361663836393865af'
                      '6576656e745f74696d657374616d70b8313937302d30312d30315430'
                      '303a30303a30302e3030305aab6f726465725f6576656e74ae6f7264'
                      '65725f6d6f646966696564af6f726465725f69645f62726f6b6572a7'
                      '42313233343536ae6d6f6469666965645f7072696365a132ad6d6f64'
                      '69666965645f74696d65b8313937302d30312d30315430303a30303a'
                      '30302e3030305a')

        body = bytes.fromhex(hex_string)

        # Act
        result = serializer.deserialize(body)

        # Assert - Warnings can be ignored (PyCharm doesn't know the type).
        self.assertTrue(isinstance(result, OrderModified))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual(OrderId('StubOrderId'), result.order_id)
        self.assertEqual(OrderId('B123456'), result.broker_order_id)
        self.assertEqual(Decimal('2'), result.modified_price)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.modified_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_expired_events_from_csharp(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Hex bytes string from C# MsgPack.Cli
        hex_string = ('87aa6576656e745f74797065ab6f726465725f6576656e74a673796d'
                      '626f6cab4155445553442e4658434da86f726465725f6964ab537475'
                      '624f726465724964a86576656e745f6964d92431313037646466372d'
                      '646633302d343566312d386365632d393538336334396332396261af'
                      '6576656e745f74696d657374616d70b8313937302d30312d30315430'
                      '303a30303a30302e3030305aab6f726465725f6576656e74ad6f7264'
                      '65725f65787069726564ac657870697265645f74696d65b831393730'
                      '2d30312d30315430303a30303a30302e3030305a')

        body = bytes.fromhex(hex_string)

        # Act
        result = serializer.deserialize(body)

        # Assert - Warning can be ignored (PyCharm doesn't know the type).
        self.assertTrue(isinstance(result, OrderExpired))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual(OrderId('StubOrderId'), result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.expired_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_partially_filled_events_from_csharp(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Hex bytes string from C# MsgPack.Cli
        hex_string = ('8daa6576656e745f74797065ab6f726465725f6576656e74a673796d'
                      '626f6cab4155445553442e4658434da86f726465725f6964ab537475'
                      '624f726465724964a86576656e745f6964d92439336433303533342d'
                      '623235392d346435302d613839372d373237356433303064316331af'
                      '6576656e745f74696d657374616d70b8313937302d30312d30315430'
                      '303a30303a30302e3030305aab6f726465725f6576656e74b66f7264'
                      '65725f7061727469616c6c795f66696c6c6564ac657865637574696f'
                      '6e5f6964a745313233343536b0657865637574696f6e5f7469636b65'
                      '74a750313233343536aa6f726465725f73696465a3425559af66696c'
                      '6c65645f7175616e74697479d20000c350af6c65617665735f717561'
                      '6e74697479d20000c350ad617665726167655f7072696365a3322e30'
                      'ae657865637574696f6e5f74696d65b8313937302d30312d30315430'
                      '303a30303a30302e3030305a')

        body = bytes.fromhex(hex_string)

        # Act
        result = serializer.deserialize(body)

        # Assert - Warnings can be ignored (PyCharm doesn't know the type).
        self.assertTrue(isinstance(result, OrderPartiallyFilled))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual(OrderId('StubOrderId'), result.order_id)
        self.assertEqual(ExecutionId('E123456'), result.execution_id)
        self.assertEqual(ExecutionTicket('P123456'), result.execution_ticket)
        self.assertEqual(OrderSide.BUY, result.order_side)
        self.assertEqual(50000, result.filled_quantity)
        self.assertEqual(50000, result.leaves_quantity)
        self.assertEqual(Decimal('2'), result.average_price)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.execution_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_filled_events_from_csharp(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Hex bytes string from C# MsgPack.Cli
        hex_string = ('8caa6576656e745f74797065ab6f726465725f6576656e74a673796d'
                      '626f6cab4155445553442e4658434da86f726465725f6964ab537475'
                      '624f726465724964a86576656e745f6964d92464393731363331302d'
                      '636231382d346338352d393134382d356263303462353733343935af'
                      '6576656e745f74696d657374616d70b8313937302d30312d30315430'
                      '303a30303a30302e3030305aab6f726465725f6576656e74ac6f7264'
                      '65725f66696c6c6564ac657865637574696f6e5f6964a74531323334'
                      '3536b0657865637574696f6e5f7469636b6574a750313233343536aa'
                      '6f726465725f73696465a3425559af66696c6c65645f7175616e7469'
                      '7479d2000186a0ad617665726167655f7072696365a3322e30ae6578'
                      '65637574696f6e5f74696d65b8313937302d30312d30315430303a30'
                      '303a30302e3030305a')

        body = bytes.fromhex(hex_string)

        # Act
        result = serializer.deserialize(body)

        # Assert - Warnings can be ignored (PyCharm doesn't know the type).
        self.assertTrue(isinstance(result, OrderFilled))
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual(OrderId('StubOrderId'), result.order_id)
        self.assertEqual(ExecutionId('E123456'), result.execution_id)
        self.assertEqual(ExecutionTicket('P123456'), result.execution_ticket)
        self.assertEqual(OrderSide.BUY, result.order_side)
        self.assertEqual(100000, result.filled_quantity)
        self.assertEqual(Decimal('2'), result.average_price)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.execution_time)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_account_events_from_csharp(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Hex bytes string from C# MsgPack.Cli
        hex_string = ('8eaa6576656e745f74797065ad6163636f756e745f6576656e74aa61'
                      '63636f756e745f6964ab4658434d2d313233343536a662726f6b6572'
                      'a44658434dae6163636f756e745f6e756d626572a6313233343536a8'
                      '63757272656e6379a3555344ac636173685f62616c616e6365a63130'
                      '30303030ae636173685f73746172745f646179a6313030303030b163'
                      '6173685f61637469766974795f646179a130b76d617267696e5f7573'
                      '65645f6c69717569646174696f6ea130b76d617267696e5f75736564'
                      '5f6d61696e74656e616e6365a130ac6d617267696e5f726174696fa1'
                      '30b26d617267696e5f63616c6c5f737461747573a0a86576656e745f'
                      '6964d92438303335363063642d656566622d343962622d616338652d'
                      '653766303337383638393938af6576656e745f74696d657374616d70'
                      'b8313937302d30312d30315430303a30303a30302e3030305a')

        body = bytes.fromhex(hex_string)

        # Act
        result = serializer.deserialize(body)

        # Assert - Warnings can be ignored (PyCharm doesn't know the type).
        self.assertTrue(isinstance(result, AccountEvent))
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)


class InstrumentSerializerTests(unittest.TestCase):

    def test_can_serialize_and_deserialize_instrument(self):
        # Arrange
        serializer = InstrumentSerializer()

        instrument = Instrument(
            Symbol('AUDUSD', Venue.FXCM),
            'AUD/USD',
            CurrencyCode.USD,
            SecurityType.FOREX,
            5,
            Decimal('0.00001'),
            Decimal('1'),
            Decimal('0'),
            1,
            1,
            0,
            0,
            0,
            0,
            1,
            50000000,
            Decimal('9'),
            Decimal('1.1'),
            Decimal('-1.1'),
            datetime.now(timezone.utc))

        # serialized = serializer.serialize(instrument)
        # print(serialized)
