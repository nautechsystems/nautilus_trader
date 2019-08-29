# -------------------------------------------------------------------------------------------------
# <copyright file="test_serialization_serializers.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from base64 import b64encode, b64decode

from nautilus_trader.core.types import *
from nautilus_trader.common.account import *
from nautilus_trader.common.clock import *
from nautilus_trader.model.commands import *
from nautilus_trader.model.identifiers import *
from nautilus_trader.model.objects import *
from nautilus_trader.model.order import *
from nautilus_trader.model.events import *
from nautilus_trader.serialization.data import *
from nautilus_trader.serialization.serializers import *
from nautilus_trader.serialization.common import *
from nautilus_trader.network.requests import *
from nautilus_trader.network.responses import *
from test_kit.stubs import *

UNIX_EPOCH = TestStubs.unix_epoch()


class MsgPackOrderSerializerTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.serializer = MsgPackOrderSerializer()
        self.order_factory = OrderFactory(
            id_tag_trader=IdTag('001'),
            id_tag_strategy=IdTag('001'),
            clock=TestClock())
        print('\n')

    def test_can_serialize_and_deserialize_market_orders(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Label('U1_E'),)

        # Act
        serialized = self.serializer.serialize(order)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(order, deserialized)
        print('market')
        print(b64encode(serialized))

    def test_can_serialize_and_deserialize_limit_orders(self):
        # Arrange
        order = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(1.00000, 5),
            Label('S1_SL'),
            TimeInForce.DAY,
            expire_time=None)

        # Act
        serialized = self.serializer.serialize(order)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(order, deserialized)
        print(b64encode(serialized))
        print(order)

    def test_can_serialize_and_deserialize_limit_orders_with_expire_time(self):
        # Arrange
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
        serialized = self.serializer.serialize(order)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(order, deserialized)
        print(b64encode(serialized))
        print(order)

    def test_can_serialize_and_deserialize_stop_limit_orders(self):
        # Arrange
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
        serialized = self.serializer.serialize(order)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(order, deserialized)
        print(b64encode(serialized))
        print(order)

    def test_can_serialize_and_deserialize_stop_limit_orders_with_expire_time(self):
        # Arrange
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
        serialized = self.serializer.serialize(order)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(order, deserialized)
        print(b64encode(serialized))
        print(order)


class MsgPackCommandSerializerTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.account = Account()
        self.serializer = MsgPackCommandSerializer()
        self.order_factory = OrderFactory(
            id_tag_trader=IdTag('001'),
            id_tag_strategy=IdTag('001'),
            clock=TestClock())
        print('\n')

    def test_can_serialize_and_deserialize_account_inquiry_command(self):
        # Arrange
        command = AccountInquiry(
            self.account.id,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, command)
        print(b64encode(serialized))
        print(command)

    def test_can_serialize_and_deserialize_submit_order_commands(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        command = SubmitOrder(
            TraderId('TESTER', '000'),
            StrategyId('SCALPER', '01'),
            self.account.id,
            PositionId('123456'),
            order,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(command, deserialized)
        self.assertEqual(order, deserialized.order)
        print(b64encode(serialized))
        print(command)

    def test_can_serialize_and_deserialize_submit_atomic_order_no_take_profit_commands(self):
        # Arrange
        atomic_order = self.order_factory.atomic_market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('0.99900'))

        command = SubmitAtomicOrder(
            TraderId('TESTER', '000'),
            StrategyId('SCALPER', '01'),
            self.account.id,
            PositionId('123456'),
            atomic_order,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(command, deserialized)
        self.assertEqual(atomic_order, deserialized.atomic_order)
        print(b64encode(serialized))
        print(command)

    def test_can_serialize_and_deserialize_submit_atomic_order_with_take_profit_commands(self):
        # Arrange
        atomic_order = self.order_factory.atomic_limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price('0.99900'),
            Price('1.00000'),
            Price('1.00010'))

        command = SubmitAtomicOrder(
            TraderId('TESTER', '000'),
            StrategyId('SCALPER', '01'),
            self.account.id,
            PositionId('123456'),
            atomic_order,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(command, deserialized)
        self.assertEqual(atomic_order, deserialized.atomic_order)
        print(b64encode(serialized))
        print(command)

    def test_can_serialize_and_deserialize_cancel_order_commands(self):
        # Arrange
        command = CancelOrder(
            TraderId('TESTER', '000'),
            StrategyId('SCALPER', '01'),
            self.account.id,
            OrderId('O-123456'),
            ValidString('EXPIRED'),
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(command, deserialized)
        print(b64encode(serialized))
        print(command)

    def test_can_serialize_and_deserialize_modify_order_commands(self):
        # Arrange
        command = ModifyOrder(
            TraderId('TESTER', '000'),
            StrategyId('SCALPER', '01'),
            self.account.id,
            OrderId('O-123456'),
            Price('1.00001'),
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(command, deserialized)
        print(b64encode(serialized))
        print(command)


class MsgPackEventSerializerTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.account = Account()
        self.serializer = MsgPackEventSerializer()

    def test_can_serialize_and_deserialize_order_initialized_events(self):
        # Arrange
        event = OrderInitialized(
            OrderId('O-123456'),
            AUDUSD_FXCM,
            None,
            OrderSide.SELL,
            OrderType.STOP_LIMIT,
            Quantity(100000),
            Price('1.50000'),
            TimeInForce.DAY,
            None,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialize_and_deserialize_order_submitted_events(self):
        # Arrange
        event = OrderSubmitted(
            OrderId('O-123456'),
            self.account.id,
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialize_and_deserialize_order_accepted_events(self):
        # Arrange
        event = OrderAccepted(
            OrderId('O-123456'),
            self.account.id,
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialize_and_deserialize_order_rejected_events(self):
        # Arrange
        event = OrderRejected(
            OrderId('O-123456'),
            self.account.id,
            UNIX_EPOCH,
            ValidString('ORDER_ID_INVALID'),
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialize_and_deserialize_order_working_events(self):
        # Arrange
        event = OrderWorking(
            OrderId('O-123456'),
            OrderId('BO-123456'),
            self.account.id,
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
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialize_and_deserialize_order_working_events_with_expire_time(self):
        # Arrange
        event = OrderWorking(
            OrderId('O-123456'),
            OrderId('BO-123456'),
            self.account.id,
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
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialize_and_deserialize_order_cancelled_events(self):
        # Arrange
        event = OrderCancelled(
            OrderId('O-123456'),
            self.account.id,
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialize_and_deserialize_order_cancel_reject_events(self):
        # Arrange
        event = OrderCancelReject(
            OrderId('O-123456'),
            self.account.id,
            UNIX_EPOCH,
            ValidString('RESPONSE'),
            ValidString('ORDER_DOES_NOT_EXIST'),
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialize_and_deserialize_order_modified_events(self):
        # Arrange
        event = OrderModified(
            OrderId('O-123456'),
            OrderId('BO-123456'),
            self.account.id,
            Price('0.80010'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialize_and_deserialize_order_expired_events(self):
        # Arrange
        event = OrderExpired(
            OrderId('O-123456'),
            self.account.id,
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialize_and_deserialize_order_partially_filled_events(self):
        # Arrange
        event = OrderPartiallyFilled(
            OrderId('O-123456'),
            self.account.id,
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
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialize_and_deserialize_order_filled_events(self):
        # Arrange
        event = OrderFilled(
            OrderId('O-123456'),
            self.account.id,
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
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_deserialize_account_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'jKRUeXBlsUFjY291bnRTdGF0ZUV2ZW50oklk2SQ3Yjc4YTI4Ny05ZjY4LTQzY2UtYjhmMy01M2NlNmY3Y2M0NjKpVGltZXN0YW1wuDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWqlBY2NvdW50SWSsRlhDTS1EMTIzNDU2qEN1cnJlbmN5o1VTRKtDYXNoQmFsYW5jZaYxMDAwMDCsQ2FzaFN0YXJ0RGF5pjEwMDAwMK9DYXNoQWN0aXZpdHlEYXmhMLVNYXJnaW5Vc2VkTGlxdWlkYXRpb26hMLVNYXJnaW5Vc2VkTWFpbnRlbmFuY2WhMKtNYXJnaW5SYXRpb6EwsE1hcmdpbkNhbGxTdGF0dXOg'
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, AccountStateEvent))
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_submitted_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'hqRUeXBlrk9yZGVyU3VibWl0dGVkoklk2SRkYmVjNzU4OS00MjEwLTQ4Y2EtOTE2ZS01ZWM0YzliZGMyOTepVGltZXN0YW1wuDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWqdPcmRlcklkqE8tMTIzNDU2qUFjY291bnRJZK1GWENNLTAyODUxOTA4rVN1Ym1pdHRlZFRpbWW4MTk3MC0wMS0wMVQwMDowMDowMC4wMDBa'
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderSubmitted))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(AccountId('FXCM', '02851908'), result.account_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.submitted_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_accepted_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'hqRUeXBlrU9yZGVyQWNjZXB0ZWSiSWTZJGQyYjY5Y2U4LWM0MGQtNGUwMi1hOGZmLTQxYTFlYWI2NDZhM6lUaW1lc3RhbXC4MTk3MC0wMS0wMVQwMDowMDowMC4wMDBap09yZGVySWSoTy0xMjM0NTapQWNjb3VudElkrUZYQ00tMDI4NTE5MDisQWNjZXB0ZWRUaW1luDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWg=='
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderAccepted))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(AccountId('FXCM', '02851908'), result.account_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.accepted_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_rejected_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'h6RUeXBlrU9yZGVyUmVqZWN0ZWSiSWTZJDIzZjJiMDE3LWQxY2QtNGFjMS1hYmIwLTRkNTRmNWMxNGYwM6lUaW1lc3RhbXC4MTk3MC0wMS0wMVQwMDowMDowMC4wMDBap09yZGVySWSoTy0xMjM0NTapQWNjb3VudElkrUZYQ00tMDI4NTE5MDisUmVqZWN0ZWRUaW1luDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWq5SZWplY3RlZFJlYXNvbq1JTlZBTElEX09SREVS'
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderRejected))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.rejected_time)
        self.assertEqual('INVALID_ORDER', result.rejected_reason.value)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_working_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'j6RUeXBlrE9yZGVyV29ya2luZ6JJZNkkY2VjZTQ5OGEtNWViNy00OTQ1LWJiODQtMzFlMmU0OWMyMWI3qVRpbWVzdGFtcLgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFqnT3JkZXJJZKhPLTEyMzQ1Nq1PcmRlcklkQnJva2VyqUJPLTEyMzQ1NqlBY2NvdW50SWStRlhDTS0wMjg1MTkwOKZTeW1ib2yrQVVEVVNELkZYQ02lTGFiZWyhRalPcmRlclNpZGWjQlVZqU9yZGVyVHlwZatTVE9QX01BUktFVKhRdWFudGl0eQGlUHJpY2WjMS4wq1RpbWVJbkZvcmNlo0RBWapFeHBpcmVUaW1lpE5PTkWrV29ya2luZ1RpbWW4MTk3MC0wMS0wMVQwMDowMDowMC4wMDBa'
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderWorking))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(OrderId('BO-123456'), result.order_id_broker)
        self.assertEqual(AccountId('FXCM', '02851908'), result.account_id)
        self.assertEqual(Symbol('AUDUSD', Venue('FXCM')), result.symbol)
        self.assertEqual(Label('E'), result.label)
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
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'j6RUeXBlrE9yZGVyV29ya2luZ6JJZNkkODkwNjQ0ZTItZWY4Yy00YzU3LTk5MjgtYjA2YmU0MzBlNzFlqVRpbWVzdGFtcLgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFqnT3JkZXJJZKhPLTEyMzQ1Nq1PcmRlcklkQnJva2VyqUJPLTEyMzQ1NqlBY2NvdW50SWStRlhDTS0wMjg1MTkwOKZTeW1ib2yrQVVEVVNELkZYQ02lTGFiZWyhRalPcmRlclNpZGWjQlVZqU9yZGVyVHlwZatTVE9QX01BUktFVKhRdWFudGl0eQGlUHJpY2WjMS4wq1RpbWVJbkZvcmNlo0dURKpFeHBpcmVUaW1luDE5NzAtMDEtMDFUMDA6MDE6MDAuMDAwWqtXb3JraW5nVGltZbgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFo='
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderWorking))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(OrderId('BO-123456'), result.order_id_broker)
        self.assertEqual(AccountId('FXCM', '02851908'), result.account_id)
        self.assertEqual(Symbol('AUDUSD', Venue('FXCM')), result.symbol)
        self.assertEqual(Label('E'), result.label)
        self.assertEqual(OrderSide.BUY, result.order_side)
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
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'hqRUeXBlrk9yZGVyQ2FuY2VsbGVkoklk2SRlOTJlNDlkYi1mZGFlLTQ0ZTctODY0Yy04NmFhMmZhYmI3ZjapVGltZXN0YW1wuDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWqdPcmRlcklkqE8tMTIzNDU2qUFjY291bnRJZK1GWENNLTAyODUxOTA4rUNhbmNlbGxlZFRpbWW4MTk3MC0wMS0wMVQwMDowMDowMC4wMDBa'
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderCancelled))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(AccountId('FXCM', '02851908'), result.account_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.cancelled_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_cancel_reject_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'iKRUeXBlsU9yZGVyQ2FuY2VsUmVqZWN0oklk2SQ1YTJjOWQzNi0yZDFmLTQxNTctYWQ5My02ZTBlNjM3ZGIyYTSpVGltZXN0YW1wuDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWqdPcmRlcklkqE8tMTIzNDU2qUFjY291bnRJZK1GWENNLTAyODUxOTA4rFJlamVjdGVkVGltZbgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFqyUmVqZWN0ZWRSZXNwb25zZVRvsFJFSkVDVF9SRVNQT05TRT+uUmVqZWN0ZWRSZWFzb26vT1JERVJfTk9UX0ZPVU5E'
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderCancelReject))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(AccountId('FXCM', '02851908'), result.account_id)
        self.assertEqual('REJECT_RESPONSE?', result.rejected_response_to.value)
        self.assertEqual('ORDER_NOT_FOUND', result.rejected_reason.value)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.rejected_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_modified_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'iKRUeXBlrU9yZGVyTW9kaWZpZWSiSWTZJDg1MzI4NGM1LTIxN2EtNDEyYS04YzljLTZmZDlhODIxZGYxYqlUaW1lc3RhbXC4MTk3MC0wMS0wMVQwMDowMDowMC4wMDBap09yZGVySWSoTy0xMjM0NTatT3JkZXJJZEJyb2tlcqlCTy0xMjM0NTapQWNjb3VudElkrUZYQ00tMDI4NTE5MDitTW9kaWZpZWRQcmljZaEyrE1vZGlmaWVkVGltZbgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFo='
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderModified))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(OrderId('BO-123456'), result.order_id_broker)
        self.assertEqual(AccountId('FXCM', '02851908'), result.account_id)
        self.assertEqual(Price('2'), result.modified_price)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.modified_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_expired_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'hqRUeXBlrE9yZGVyRXhwaXJlZKJJZNkkNTJmOGM1OTctYjA5Zi00YzYzLWE5NWEtYzQzYzExNjEzYzQ3qVRpbWVzdGFtcLgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFqnT3JkZXJJZKhPLTEyMzQ1NqlBY2NvdW50SWStRlhDTS0wMjg1MTkwOKtFeHBpcmVkVGltZbgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFo='
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderExpired))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(AccountId('FXCM', '02851908'), result.account_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.expired_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_partially_filled_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'jaRUeXBltE9yZGVyUGFydGlhbGx5RmlsbGVkoklk2SRmODRjM2RkNS1jZTBjLTQyNTctOTQ1Ni0yMzZhNmViYmIyYzCpVGltZXN0YW1wuDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWqdPcmRlcklkqE8tMTIzNDU2qUFjY291bnRJZK1GWENNLTAyODUxOTA4q0V4ZWN1dGlvbklkp0UxMjM0NTavRXhlY3V0aW9uVGlja2V0p1AxMjM0NTamU3ltYm9sq0FVRFVTRC5GWENNqU9yZGVyU2lkZaNCVVmuRmlsbGVkUXVhbnRpdHnSAADDUK5MZWF2ZXNRdWFudGl0edIAAMNQrEF2ZXJhZ2VQcmljZaMyLjCtRXhlY3V0aW9uVGltZbgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFo='
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderPartiallyFilled))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(AccountId('FXCM', '02851908'), result.account_id)
        self.assertEqual(ExecutionId('E123456'), result.execution_id)
        self.assertEqual(ExecutionTicket('P123456'), result.execution_ticket)
        self.assertEqual(Symbol('AUDUSD', Venue('FXCM')), result.symbol)
        self.assertEqual(OrderSide.BUY, result.order_side)
        self.assertEqual(Quantity(50000), result.filled_quantity)
        self.assertEqual(Quantity(50000), result.leaves_quantity)
        self.assertEqual(Price('2'), result.average_price)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.execution_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_filled_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'jKRUeXBlq09yZGVyRmlsbGVkoklk2SRlNjc4N2I1ZS1lMDQzLTQyNGItYmU4Yy1lMGVkYjNhODIyM2SpVGltZXN0YW1wuDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWqdPcmRlcklkqE8tMTIzNDU2qUFjY291bnRJZK1GWENNLTAyODUxOTA4q0V4ZWN1dGlvbklkp0UxMjM0NTavRXhlY3V0aW9uVGlja2V0p1AxMjM0NTamU3ltYm9sq0FVRFVTRC5GWENNqU9yZGVyU2lkZaNCVVmuRmlsbGVkUXVhbnRpdHnSAAGGoKxBdmVyYWdlUHJpY2WjMi4wrUV4ZWN1dGlvblRpbWW4MTk3MC0wMS0wMVQwMDowMDowMC4wMDBa'
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderFilled))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(AccountId('FXCM', '02851908'), result.account_id)
        self.assertEqual(ExecutionId('E123456'), result.execution_id)
        self.assertEqual(ExecutionTicket('P123456'), result.execution_ticket)
        self.assertEqual(Symbol('AUDUSD', Venue('FXCM')), result.symbol)
        self.assertEqual(OrderSide.BUY, result.order_side)
        self.assertEqual(Quantity(100000), result.filled_quantity)
        self.assertEqual(Price('2'), result.average_price)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.execution_time)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)


class MsgPackInstrumentSerializerTests(unittest.TestCase):

    def test_can_serialize_and_deserialize_instrument(self):
        # Arrange
        serializer = BsonInstrumentSerializer()

        instrument = Instrument(
            instrument_id=InstrumentId('AUDUSD.FXCM'),
            symbol=Symbol('AUDUSD', Venue('FXCM')),
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


class MsgPackRequestSerializerTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.serializer = MsgPackRequestSerializer()

    def test_can_serialize_and_deserialize_tick_data_requests(self):
        # Arrange
        query = {
            "DataType": "Tick[]",
            "Symbol": "AUDUSD.FXCM",
            "FromDateTime": convert_datetime_to_string(UNIX_EPOCH),
            "ToDateTime": convert_datetime_to_string(UNIX_EPOCH),
        }

        request = DataRequest(
            query,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(request)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertTrue(isinstance(deserialized, DataRequest))
        self.assertEqual("Tick[]", deserialized.query["DataType"])

    def test_can_serialize_and_deserialize_bar_data_requests(self):
        # Arrange
        query = {
            "DataType": "Bar[]",
            "Symbol": "AUDUSD.FXCM",
            "Specification": "1-MIN[BID]",
            "FromDateTime": convert_datetime_to_string(UNIX_EPOCH),
            "ToDateTime": convert_datetime_to_string(UNIX_EPOCH),
        }

        request = DataRequest(
            query,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(request)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertTrue(isinstance(deserialized, DataRequest))
        self.assertEqual("Bar[]", deserialized.query["DataType"])

    def test_can_serialize_and_deserialize_instrument_requests(self):
        # Arrange
        query = {
            "DataType": "Instrument",
            "Symbol": "AUDUSD.FXCM",
        }

        request = DataRequest(
            query,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(request)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertTrue(isinstance(deserialized, DataRequest))
        self.assertEqual("Instrument", deserialized.query["DataType"])

    def test_can_serialize_and_deserialize_instruments_requests(self):
        # Arrange
        query = {
            "DataType": "Instrument[]",
            "Symbol": "FXCM",
        }

        request = DataRequest(
            query,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(request)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertTrue(isinstance(deserialized, DataRequest))
        self.assertEqual("Instrument[]", deserialized.query["DataType"])


class MsgPackResponseSerializerTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.serializer = MsgPackResponseSerializer()

    def test_can_serialize_and_deserialize_data_responses(self):
        # Arrange
        data = b'\x01 \x00'
        data_encoding = 'BSON1.1'

        response = DataResponse(
            data=data,
            data_encoding=data_encoding,
            correlation_id=GUID(uuid.uuid4()),
            response_id=GUID(uuid.uuid4()),
            response_timestamp=UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(response)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertTrue(isinstance(deserialized, DataResponse))
        self.assertEqual(data, deserialized.data)
        self.assertEqual(data_encoding, deserialized.data_encoding)

        print(deserialized)
