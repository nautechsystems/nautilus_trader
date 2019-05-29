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

from base64 import b64encode, b64decode
from datetime import datetime, timezone

from inv_trader.common.clock import TestClock
from inv_trader.commands import SubmitOrder, SubmitAtomicOrder, CancelOrder, ModifyOrder
from inv_trader.commands import CollateralInquiry
from inv_trader.model.enums import *
from inv_trader.model.identifiers import *
from inv_trader.model.objects import *
from inv_trader.model.order import *
from inv_trader.model.events import *
from inv_trader.serialization import MsgPackOrderSerializer
from inv_trader.serialization import MsgPackCommandSerializer
from inv_trader.serialization import MsgPackEventSerializer
from inv_trader.serialization import MsgPackInstrumentSerializer
from inv_trader.common.serialization import convert_price_to_string, convert_datetime_to_string
from inv_trader.common.serialization import convert_string_to_price, convert_string_to_datetime
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = Symbol('AUDUSD', Venue.FXCM)
GBPUSD_FXCM = Symbol('GBPUSD', Venue.FXCM)


class SerializationFunctionTests(unittest.TestCase):

    def test_can_convert_price_to_string_from_none(self):
        # Arrange
        # Act
        result = convert_price_to_string(None)

        # Assert
        self.assertEqual('NONE', result)

    def test_can_convert_price_to_string_from_decimal(self):
        # Arrange
        # Act
        result = convert_price_to_string(Price('1.00000'))

        # Assert
        self.assertEqual('1.00000', result)

    def test_can_convert_string_to_price_from_none(self):
        # Arrange
        # Act
        result = convert_string_to_price('NONE')

        # Assert
        self.assertEqual(None, result)

    def test_can_convert_string_to_price_from_decimal(self):
        # Arrange
        # Act
        result = convert_string_to_price('1.00000')

        # Assert
        self.assertEqual(Price('1.00000'), result)

    def test_can_convert_datetime_to_string_from_none(self):
        # Arrange
        # Act
        result = convert_datetime_to_string(None)

        # Assert
        self.assertEqual('NONE', result)

    def test_can_convert_datetime_to_string(self):
        # Arrange
        # Act
        result = convert_datetime_to_string(UNIX_EPOCH)

        # Assert
        self.assertEqual('1970-01-01T00:00:00.000Z', result)

    def test_can_convert_string_to_time_from_datetime(self):
        # Arrange
        # Act
        result = convert_string_to_datetime('1970-01-01T00:00:00.000Z')

        # Assert
        self.assertEqual(UNIX_EPOCH, result)

    def test_can_convert_string_to_time_from_none(self):
        # Arrange
        # Act
        result = convert_string_to_datetime('NONE')

        # Assert
        self.assertEqual(None, result)


class MsgPackOrderSerializerTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.order_factory = OrderFactory(
            id_tag_trader='001',
            id_tag_strategy='001',
            clock=TestClock())
        print('\n')

    def test_can_serialize_and_deserialize_market_orders(self):
        # Arrange
        serializer = MsgPackOrderSerializer()

        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Label('U1_E'),)

        # Act
        serialized = serializer.serialize(order)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(order, deserialized)
        print('market')
        print(b64encode(serialized))

    def test_can_serialize_and_deserialize_limit_orders(self):
        # Arrange
        serializer = MsgPackOrderSerializer()

        order = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(1.00000, 5),
            Label('S1_SL'),
            TimeInForce.DAY,
            expire_time=None)

        # Act
        serialized = serializer.serialize(order)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(order, deserialized)
        print(b64encode(serialized))
        print(order)

    def test_can_serialize_and_deserialize_limit_orders_with_expire_time(self):
        # Arrange
        serializer = MsgPackOrderSerializer()

        order = Order(
            OrderId('O123456'),
            AUDUSD_FXCM,
            OrderSide.BUY,
            OrderType.LIMIT,
            Quantity(100000),
            UNIX_EPOCH,
            Price('1.00000'),
            label=None,
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(order)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(order, deserialized)
        print(b64encode(serialized))
        print(order)

    def test_can_serialize_and_deserialize_stop_limit_orders(self):
        # Arrange
        serializer = MsgPackOrderSerializer()

        order = Order(
            OrderId('O123456'),
            AUDUSD_FXCM,
            OrderSide.BUY,
            OrderType.STOP_LIMIT,
            Quantity(100000),
            UNIX_EPOCH,
            Price('1.00000'),
            Label('S1_SL'))

        # Act
        serialized = serializer.serialize(order)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(order, deserialized)
        print(b64encode(serialized))
        print(order)

    def test_can_serialize_and_deserialize_stop_limit_orders_with_expire_time(self):
        # Arrange
        serializer = MsgPackOrderSerializer()

        order = Order(
            OrderId('O123456'),
            AUDUSD_FXCM,
            OrderSide.BUY,
            OrderType.STOP_LIMIT,
            Quantity(100000),
            UNIX_EPOCH,
            Price('1.00000'),
            label=None,
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(order)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(order, deserialized)
        print(b64encode(serialized))
        print(order)


class MsgPackCommandSerializerTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.order_factory = OrderFactory(
            id_tag_trader='001',
            id_tag_strategy='001',
            clock=TestClock())
        print('\n')

    def test_can_serialize_and_deserialize_submit_order_commands(self):
        # Arrange
        serializer = MsgPackCommandSerializer()

        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        command = SubmitOrder(
            TraderId('Trader-001'),
            StrategyId('SCALPER01'),
            PositionId('123456'),
            order,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(command)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(command, deserialized)
        self.assertEqual(order, deserialized.order)
        print(b64encode(serialized))
        print(command)

    def test_can_serialize_and_deserialize_submit_atomic_order_no_take_profit_commands(self):
        # Arrange
        serializer = MsgPackCommandSerializer()

        atomic_order = self.order_factory.atomic_market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('0.99900'))

        command = SubmitAtomicOrder(
            TraderId('Trader-001'),
            StrategyId('SCALPER01'),
            PositionId('123456'),
            atomic_order,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(command)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(command, deserialized)
        self.assertEqual(atomic_order, deserialized.atomic_order)
        print(b64encode(serialized))
        print(command)

    def test_can_serialize_and_deserialize_submit_atomic_order_with_take_profit_commands(self):
        # Arrange
        serializer = MsgPackCommandSerializer()

        atomic_order = self.order_factory.atomic_limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('0.99900'),
            Price('1.00000'),
            Price('1.00010'))

        command = SubmitAtomicOrder(
            TraderId('Trader-001'),
            StrategyId('SCALPER01'),
            PositionId('123456'),
            atomic_order,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(command)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(command, deserialized)
        self.assertEqual(atomic_order, deserialized.atomic_order)
        print(b64encode(serialized))
        print(command)

    def test_can_serialize_and_deserialize_cancel_order_commands(self):
        # Arrange
        serializer = MsgPackCommandSerializer()

        command = CancelOrder(
            TraderId('Trader-001'),
            StrategyId('SCALPER01'),
            OrderId('O-123456'),
            ValidString('EXPIRED'),
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(command)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(command, deserialized)
        print(b64encode(serialized))
        print(command)

    def test_can_serialize_and_deserialize_modify_order_commands(self):
        # Arrange
        serializer = MsgPackCommandSerializer()

        command = ModifyOrder(
            TraderId('Trader-001'),
            StrategyId('SCALPER01'),
            OrderId('O-123456'),
            Price('1.00001'),
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(command)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(command, deserialized)
        print(b64encode(serialized))
        print(command)

    def test_can_serialize_and_deserialize_collateral_inquiry_command(self):
        # Arrange
        serializer = MsgPackCommandSerializer()

        command = CollateralInquiry(GUID(uuid.uuid4()), UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(command)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, command)
        print(b64encode(serialized))
        print(command)


class MsgPackEventSerializerTests(unittest.TestCase):

    def test_can_serialize_and_deserialize_order_submitted_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        event = OrderSubmitted(
            OrderId('O-123456'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(event)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialize_and_deserialize_order_accepted_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        event = OrderAccepted(
            OrderId('O-123456'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(event)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialize_and_deserialize_order_rejected_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        event = OrderRejected(
            OrderId('O-123456'),
            UNIX_EPOCH,
            ValidString('ORDER_ID_INVALID'),
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(event)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialize_and_deserialize_order_working_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        event = OrderWorking(
            OrderId('O-123456'),
            OrderId('BO-123456'),
            AUDUSD_FXCM,
            Label('S1_PT'),
            OrderSide.SELL,
            OrderType.STOP_LIMIT,
            Quantity(100000),
            Price('1.50000'),
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

    def test_can_serialize_and_deserialize_order_working_events_with_expire_time(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        event = OrderWorking(
            OrderId('O-123456'),
            OrderId('BO-123456'),
            AUDUSD_FXCM,
            Label('S1_PT'),
            OrderSide.SELL,
            OrderType.STOP_LIMIT,
            Quantity(100000),
            Price('1.50000'),
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

    def test_can_serialize_and_deserialize_order_cancelled_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        event = OrderCancelled(
            OrderId('O-123456'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(event)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialize_and_deserialize_order_cancel_reject_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        event = OrderCancelReject(
            OrderId('O-123456'),
            UNIX_EPOCH,
            ValidString('RESPONSE'),
            ValidString('ORDER_DOES_NOT_EXIST'),
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(event)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialize_and_deserialize_order_modified_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        event = OrderModified(
            OrderId('O-123456'),
            OrderId('BO-123456'),
            Price('0.80010'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(event)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialize_and_deserialize_order_expired_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        event = OrderExpired(
            OrderId('O-123456'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(event)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialize_and_deserialize_order_partially_filled_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        event = OrderPartiallyFilled(
            OrderId('O-123456'),
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(50000),
            Quantity(50000),
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(event)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialize_and_deserialize_order_filled_events(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        event = OrderFilled(
            OrderId('O-123456'),
            ExecutionId('E123456'),
            ExecutionTicket('T123456'),
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000),
            Price('1.00000'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(event)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_deserialize_account_events_from_csharp(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'j6RUeXBlpUV2ZW50pUV2ZW50rEFjY291bnRFdmVudKdFdmVudElk2SRmMTdkYWZjMC0yZWRjLTQzZTQtOWFmZS1hOTk2M2YxZmFkYjmuRXZlbnRUaW1lc3RhbXC4MTk3MC0wMS0wMVQwMDowMDowMC4wMDBaqUFjY291bnRJZKtGWENNLTEyMzQ1NqZCcm9rZXKkRlhDTa1BY2NvdW50TnVtYmVypjEyMzQ1NqhDdXJyZW5jeaNVU0SrQ2FzaEJhbGFuY2WmMTAwMDAwrENhc2hTdGFydERheaYxMDAwMDCvQ2FzaEFjdGl2aXR5RGF5oTC1TWFyZ2luVXNlZExpcXVpZGF0aW9uoTC1TWFyZ2luVXNlZE1haW50ZW5hbmNloTCrTWFyZ2luUmF0aW+hMLBNYXJnaW5DYWxsU3RhdHVzoA=='
        body = b64decode(base64)

        # Act
        result = serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, AccountEvent))
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_submitted_events_from_csharp(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'hqRUeXBlpUV2ZW50pUV2ZW50rk9yZGVyU3VibWl0dGVkp0V2ZW50SWTZJDE2YTM3OTIxLTYzMWUtNDM0My04Yzc1LTc3YjQ2YTIyNDg2N65FdmVudFRpbWVzdGFtcLgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFqnT3JkZXJJZKhPLTEyMzQ1Nq1TdWJtaXR0ZWRUaW1luDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWg=='
        body = b64decode(base64)

        # Act
        result = serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderSubmitted))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.submitted_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_accepted_events_from_csharp(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'hqRUeXBlpUV2ZW50pUV2ZW50rU9yZGVyQWNjZXB0ZWSnRXZlbnRJZNkkNDk0OGQ2ZjMtZmRiOC00NzMxLWFkMzItYzhkMzYxOGY1MmYxrkV2ZW50VGltZXN0YW1wuDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWqdPcmRlcklkqE8tMTIzNDU2rEFjY2VwdGVkVGltZbgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFo='
        body = b64decode(base64)

        # Act
        result = serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderAccepted))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.accepted_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_rejected_events_from_csharp(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'h6RUeXBlpUV2ZW50pUV2ZW50rU9yZGVyUmVqZWN0ZWSnRXZlbnRJZNkkYzQzM2ZlOTMtZGIxMS00MTY5LWJlN2EtOWM1ZDdhM2Q3YjgzrkV2ZW50VGltZXN0YW1wuDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWqdPcmRlcklkqE8tMTIzNDU2rFJlamVjdGVkVGltZbgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFquUmVqZWN0ZWRSZWFzb26tSU5WQUxJRF9PUkRFUg=='
        body = b64decode(base64)

        # Act
        result = serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderRejected))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.rejected_time)
        self.assertEqual('INVALID_ORDER', result.rejected_reason.value)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_working_events_from_csharp(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'j6RUeXBlpUV2ZW50pUV2ZW50rE9yZGVyV29ya2luZ6dFdmVudElk2SQ4ZTE0ZWEyYS03N2Q4LTQ5MzAtYWE1NC0yNjdmOTk2N2FhMGSuRXZlbnRUaW1lc3RhbXC4MTk3MC0wMS0wMVQwMDowMDowMC4wMDBap09yZGVySWSoTy0xMjM0NTatT3JkZXJJZEJyb2tlcqlCTy0xMjM0NTamU3ltYm9sq0FVRFVTRC5GWENNpUxhYmVsqU8xMjM0NTZfRalPcmRlclNpZGWjQlVZqU9yZGVyVHlwZatTVE9QX01BUktFVKhRdWFudGl0eQGlUHJpY2WjMS4wq1RpbWVJbkZvcmNlo0RBWapFeHBpcmVUaW1lpE5PTkWrV29ya2luZ1RpbWW4MTk3MC0wMS0wMVQwMDowMDowMC4wMDBa'
        body = b64decode(base64)

        # Act
        result = serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderWorking))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(OrderId('BO-123456'), result.order_id_broker)
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual(Label('O123456_E'), result.label)
        self.assertEqual(OrderType.STOP_MARKET, result.order_type)
        self.assertEqual(Quantity(1), result.quantity)
        self.assertEqual(Price('1'), result.price)
        self.assertEqual(TimeInForce.DAY, result.time_in_force)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.working_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)
        self.assertIsNone(result.expire_time)

    def test_can_deserialize_order_working_events_with_expire_time_from_csharp(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'j6RUeXBlpUV2ZW50pUV2ZW50rE9yZGVyV29ya2luZ6dFdmVudElk2SQzZWIyZDE0Ni1mMWRlLTRmOTQtYjVlMi1jYjNiZDY1MWZjNzmuRXZlbnRUaW1lc3RhbXC4MTk3MC0wMS0wMVQwMDowMDowMC4wMDBap09yZGVySWSoTy0xMjM0NTatT3JkZXJJZEJyb2tlcqlCTy0xMjM0NTamU3ltYm9sq0FVRFVTRC5GWENNpUxhYmVsqU8xMjM0NTZfRalPcmRlclNpZGWjQlVZqU9yZGVyVHlwZatTVE9QX01BUktFVKhRdWFudGl0eQGlUHJpY2WjMS4wq1RpbWVJbkZvcmNlo0dURKpFeHBpcmVUaW1luDE5NzAtMDEtMDFUMDA6MDE6MDAuMDAwWqtXb3JraW5nVGltZbgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFo='
        body = b64decode(base64)

        # Act
        result = serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderWorking))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(OrderId('BO-123456'), result.order_id_broker)
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual(Label('O123456_E'), result.label)
        self.assertEqual(OrderType.STOP_MARKET, result.order_type)
        self.assertEqual(Quantity(1), result.quantity)
        self.assertEqual(Price('1'), result.price)
        self.assertEqual(TimeInForce.GTD, result.time_in_force)
        self.assertEqual(datetime(1970, 1, 1, 0, 0, 0, 0, timezone.utc), result.working_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 0, 0, 0, 0, timezone.utc), result.timestamp)
        self.assertEqual(datetime(1970, 1, 1, 0, 1, 0, 0, timezone.utc), result.expire_time)

    def test_can_deserialize_order_cancelled_events_from_csharp(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'hqRUeXBlpUV2ZW50pUV2ZW50rk9yZGVyQ2FuY2VsbGVkp0V2ZW50SWTZJGY5YjZkMjI0LWJkM2MtNDFhYS05ZTg4LTQxMDg0MGNlZTY3Ma5FdmVudFRpbWVzdGFtcLgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFqnT3JkZXJJZKhPLTEyMzQ1Nq1DYW5jZWxsZWRUaW1luDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWg=='
        body = b64decode(base64)

        # Act
        result = serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderCancelled))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.cancelled_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_cancel_reject_events_from_csharp(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'iKRUeXBlpUV2ZW50pUV2ZW50sU9yZGVyQ2FuY2VsUmVqZWN0p0V2ZW50SWTZJDY2MzBmMTAwLTMwYzktNGM0OC1iNjNhLTY0ODIxMmFiODAwOK5FdmVudFRpbWVzdGFtcLgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFqnT3JkZXJJZKhPLTEyMzQ1NqxSZWplY3RlZFRpbWW4MTk3MC0wMS0wMVQwMDowMDowMC4wMDBasFJlamVjdGVkUmVzcG9uc2WwUkVKRUNUX1JFU1BPTlNFP65SZWplY3RlZFJlYXNvbq9PUkRFUl9OT1RfRk9VTkQ='
        body = b64decode(base64)

        # Act
        result = serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderCancelReject))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual('REJECT_RESPONSE?', result.cancel_reject_response.value)
        self.assertEqual('ORDER_NOT_FOUND', result.cancel_reject_reason.value)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.cancel_reject_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_modified_events_from_csharp(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'iKRUeXBlpUV2ZW50pUV2ZW50rU9yZGVyTW9kaWZpZWSnRXZlbnRJZNkkNjA3ZmI4YzMtMTU0ZS00MDA4LTljZDAtMzA5MThjYjBjNDgwrkV2ZW50VGltZXN0YW1wuDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWqdPcmRlcklkqE8tMTIzNDU2rU9yZGVySWRCcm9rZXKpQk8tMTIzNDU2rU1vZGlmaWVkUHJpY2WhMqxNb2RpZmllZFRpbWW4MTk3MC0wMS0wMVQwMDowMDowMC4wMDBa'
        body = b64decode(base64)

        # Act
        result = serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderModified))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(OrderId('BO-123456'), result.order_id_broker)
        self.assertEqual(Price('2'), result.modified_price)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.modified_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_expired_events_from_csharp(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'hqRUeXBlpUV2ZW50pUV2ZW50rE9yZGVyRXhwaXJlZKdFdmVudElk2SQwYjMwZGJjNC1mYjYxLTQ3MTMtYjJiOS0xYmY5ZWJmY2M2YTOuRXZlbnRUaW1lc3RhbXC4MTk3MC0wMS0wMVQwMDowMDowMC4wMDBap09yZGVySWSoTy0xMjM0NTarRXhwaXJlZFRpbWW4MTk3MC0wMS0wMVQwMDowMDowMC4wMDBa'
        body = b64decode(base64)

        # Act
        result = serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderExpired))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.expired_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_partially_filled_events_from_csharp(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'jaRUeXBlpUV2ZW50pUV2ZW50tE9yZGVyUGFydGlhbGx5RmlsbGVkp0V2ZW50SWTZJDYwMTAyYThmLWE2YTUtNDdlZi1hNzgyLTg1YWUyMWVhOTc1M65FdmVudFRpbWVzdGFtcLgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFqnT3JkZXJJZKhPLTEyMzQ1NqZTeW1ib2yrQVVEVVNELkZYQ02rRXhlY3V0aW9uSWSnRTEyMzQ1Nq9FeGVjdXRpb25UaWNrZXSnUDEyMzQ1NqlPcmRlclNpZGWjQlVZrkZpbGxlZFF1YW50aXR50gAAw1CuTGVhdmVzUXVhbnRpdHnSAADDUKxBdmVyYWdlUHJpY2WjMi4wrUV4ZWN1dGlvblRpbWW4MTk3MC0wMS0wMVQwMDowMDowMC4wMDBa'
        body = b64decode(base64)

        # Act
        result = serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderPartiallyFilled))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(ExecutionId('E123456'), result.execution_id)
        self.assertEqual(ExecutionTicket('P123456'), result.execution_ticket)
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual(OrderSide.BUY, result.order_side)
        self.assertEqual(Quantity(50000), result.filled_quantity)
        self.assertEqual(Quantity(50000), result.leaves_quantity)
        self.assertEqual(Price('2'), result.average_price)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.execution_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_filled_events_from_csharp(self):
        # Arrange
        serializer = MsgPackEventSerializer()

        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'jKRUeXBlpUV2ZW50pUV2ZW50q09yZGVyRmlsbGVkp0V2ZW50SWTZJDIyMDhiZDJmLTA3MDItNGY5NC04MTUzLTg2ZmI3M2E3OGQzMK5FdmVudFRpbWVzdGFtcLgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFqnT3JkZXJJZKhPLTEyMzQ1NqZTeW1ib2yrQVVEVVNELkZYQ02rRXhlY3V0aW9uSWSnRTEyMzQ1Nq9FeGVjdXRpb25UaWNrZXSnUDEyMzQ1NqlPcmRlclNpZGWjQlVZrkZpbGxlZFF1YW50aXR50gABhqCsQXZlcmFnZVByaWNlozIuMK1FeGVjdXRpb25UaW1luDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWg=='
        body = b64decode(base64)

        # Act
        result = serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderFilled))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(ExecutionId('E123456'), result.execution_id)
        self.assertEqual(ExecutionTicket('P123456'), result.execution_ticket)
        self.assertEqual(Symbol('AUDUSD', Venue.FXCM), result.symbol)
        self.assertEqual(OrderSide.BUY, result.order_side)
        self.assertEqual(Quantity(100000), result.filled_quantity)
        self.assertEqual(Price('2'), result.average_price)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.execution_time)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)


class InstrumentSerializerTests(unittest.TestCase):

    def test_can_serialize_and_deserialize_instrument(self):
        # Arrange
        serializer = MsgPackInstrumentSerializer()

        instrument = Instrument(
            instrument_id=InstrumentId('AUDUSD.FXCM'),
            symbol=Symbol('AUDUSD', Venue.FXCM),
            broker_symbol='AUD/USD',
            quote_currency=Currency.USD,
            security_type=SecurityType.FOREX,
            tick_precision=5,
            tick_size=Decimal('0.00001'),
            round_lot_size=Quantity(1000),
            min_stop_distance_entry=0,
            min_stop_distance=0,
            min_limit_distance_entry=1,
            min_limit_distance=1,
            min_trade_size=Quantity(1),
            max_trade_size=Quantity(50000000),
            rollover_interest_buy=Decimal('1.1'),
            rollover_interest_sell=Decimal('-1.1'),
            timestamp=UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(instrument)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(instrument, deserialized)
        self.assertEqual(instrument.id, deserialized.id)
        self.assertEqual(instrument.symbol, deserialized.symbol)
        self.assertEqual(instrument.broker_symbol, deserialized.broker_symbol)
        self.assertEqual(instrument.quote_currency, deserialized.quote_currency)
        self.assertEqual(instrument.security_type, deserialized.security_type)
        self.assertEqual(instrument.tick_precision, deserialized.tick_precision)
        self.assertEqual(instrument.tick_size, deserialized.tick_size)
        self.assertEqual(instrument.round_lot_size, deserialized.round_lot_size)
        self.assertEqual(instrument.min_stop_distance_entry, deserialized.min_stop_distance_entry)
        self.assertEqual(instrument.min_stop_distance, deserialized.min_stop_distance)
        self.assertEqual(instrument.min_limit_distance_entry, deserialized.min_limit_distance_entry)
        self.assertEqual(instrument.min_limit_distance, deserialized.min_limit_distance)
        self.assertEqual(instrument.min_trade_size, deserialized.min_trade_size)
        self.assertEqual(instrument.max_trade_size, deserialized.max_trade_size)
        self.assertEqual(instrument.rollover_interest_buy, deserialized.rollover_interest_buy)
        self.assertEqual(instrument.rollover_interest_sell, deserialized.rollover_interest_sell)
        self.assertEqual(instrument.timestamp, deserialized.timestamp)
        print('instrument')
        print(b64encode(serialized))
