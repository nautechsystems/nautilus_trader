# -------------------------------------------------------------------------------------------------
# <copyright file="test_serialization_serializers.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import uuid

from base64 import b64encode, b64decode

from nautilus_trader.core.types import *
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
        self.serializer = MsgPackCommandSerializer()
        self.order_factory = OrderFactory(
            id_tag_trader=IdTag('001'),
            id_tag_strategy=IdTag('001'),
            clock=TestClock())
        print('\n')

    def test_can_serialize_and_deserialize_account_inquiry_command(self):
        # Arrange
        command = AccountInquiry(GUID(uuid.uuid4()), UNIX_EPOCH)

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
            TraderId('Trader-001'),
            StrategyId('SCALPER01'),
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
            TraderId('Trader-001'),
            StrategyId('SCALPER01'),
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
            TraderId('Trader-001'),
            StrategyId('SCALPER01'),
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
            TraderId('Trader-001'),
            StrategyId('SCALPER01'),
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
            TraderId('Trader-001'),
            StrategyId('SCALPER01'),
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
        self.serializer = MsgPackEventSerializer()

    def test_can_serialize_and_deserialize_order_submitted_events(self):
        # Arrange
        event = OrderSubmitted(
            OrderId('O-123456'),
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
        base64 = 'jqRUeXBlrEFjY291bnRFdmVudKJJZNkkNGJmY2MwYWEtZGJlZS00NzY5LTg3MzEtYTc3Y2Y5OWYxNzJkqVRpbWVzdGFtcLgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFqpQWNjb3VudElkrEZYQ00tRDEyMzQ1NqlCcm9rZXJhZ2WkRlhDTa1BY2NvdW50TnVtYmVyp0QxMjM0NTaoQ3VycmVuY3mjVVNEq0Nhc2hCYWxhbmNlpjEwMDAwMKxDYXNoU3RhcnREYXmmMTAwMDAwr0Nhc2hBY3Rpdml0eURheaEwtU1hcmdpblVzZWRMaXF1aWRhdGlvbqEwtU1hcmdpblVzZWRNYWludGVuYW5jZaEwq01hcmdpblJhdGlvoTCwTWFyZ2luQ2FsbFN0YXR1c6A='
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, AccountEvent))
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_submitted_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'haRUeXBlrk9yZGVyU3VibWl0dGVkoklk2SRiYjlkNTU4My1lMDk0LTQyMTYtOGFlOC1jMTM0OWU4ZWQyMzmpVGltZXN0YW1wuDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWqdPcmRlcklkqE8tMTIzNDU2rVN1Ym1pdHRlZFRpbWW4MTk3MC0wMS0wMVQwMDowMDowMC4wMDBa'
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderSubmitted))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.submitted_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_accepted_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'haRUeXBlrU9yZGVyQWNjZXB0ZWSiSWTZJGQ5YTBkMTc4LTY4Y2EtNGFhNS1iY2YyLTE1ODIzZjM1OWFhZqlUaW1lc3RhbXC4MTk3MC0wMS0wMVQwMDowMDowMC4wMDBap09yZGVySWSoTy0xMjM0NTasQWNjZXB0ZWRUaW1luDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWg=='
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderAccepted))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.accepted_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_rejected_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'hqRUeXBlrU9yZGVyUmVqZWN0ZWSiSWTZJDJiOWM5MWRhLWExOWYtNDkxMi1hNjllLWNhMWM3ZTRmOWU0ZKlUaW1lc3RhbXC4MTk3MC0wMS0wMVQwMDowMDowMC4wMDBap09yZGVySWSoTy0xMjM0NTasUmVqZWN0ZWRUaW1luDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWq5SZWplY3RlZFJlYXNvbq1JTlZBTElEX09SREVS'
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
        base64 = 'jqRUeXBlrE9yZGVyV29ya2luZ6JJZNkkNThhOTMwNzMtNTNiZi00MTI5LTk3N2YtODdkNThhZTI1NTNmqVRpbWVzdGFtcLgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFqnT3JkZXJJZKhPLTEyMzQ1Nq1PcmRlcklkQnJva2VyqUJPLTEyMzQ1NqZTeW1ib2yrQVVEVVNELkZYQ02lTGFiZWypTzEyMzQ1Nl9FqU9yZGVyU2lkZaNCVVmpT3JkZXJUeXBlq1NUT1BfTUFSS0VUqFF1YW50aXR5AaVQcmljZaMxLjCrVGltZUluRm9yY2WjREFZqkV4cGlyZVRpbWWkTk9ORatXb3JraW5nVGltZbgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFo='
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderWorking))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(OrderId('BO-123456'), result.order_id_broker)
        self.assertEqual(Symbol('AUDUSD', Venue('FXCM')), result.symbol)
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
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'jqRUeXBlrE9yZGVyV29ya2luZ6JJZNkkMDNkNjdhY2MtY2ZkMC00MThlLWJjMjItZWYxNDZhYWVhZjdmqVRpbWVzdGFtcLgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFqnT3JkZXJJZKhPLTEyMzQ1Nq1PcmRlcklkQnJva2VyqUJPLTEyMzQ1NqZTeW1ib2yrQVVEVVNELkZYQ02lTGFiZWypTzEyMzQ1Nl9FqU9yZGVyU2lkZaNCVVmpT3JkZXJUeXBlq1NUT1BfTUFSS0VUqFF1YW50aXR5AaVQcmljZaMxLjCrVGltZUluRm9yY2WjR1REqkV4cGlyZVRpbWW4MTk3MC0wMS0wMVQwMDowMTowMC4wMDBaq1dvcmtpbmdUaW1luDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWg=='
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderWorking))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(OrderId('BO-123456'), result.order_id_broker)
        self.assertEqual(Symbol('AUDUSD', Venue('FXCM')), result.symbol)
        self.assertEqual(Label('O123456_E'), result.label)
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
        base64 = 'haRUeXBlrk9yZGVyQ2FuY2VsbGVkoklk2SRhMjI2YmYxZC1jN2M5LTQyNGYtOWEwMi03MjllMDI3Y2M3ZWapVGltZXN0YW1wuDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWqdPcmRlcklkqE8tMTIzNDU2rUNhbmNlbGxlZFRpbWW4MTk3MC0wMS0wMVQwMDowMDowMC4wMDBa'
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderCancelled))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.cancelled_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_cancel_reject_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'h6RUeXBlsU9yZGVyQ2FuY2VsUmVqZWN0oklk2SQyNWZkM2IzZC1iOGFkLTQ4MzctOGI5Yi04MDMwOGY4ZjFkMTGpVGltZXN0YW1wuDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWqdPcmRlcklkqE8tMTIzNDU2rFJlamVjdGVkVGltZbgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFqyUmVqZWN0ZWRSZXNwb25zZVRvsFJFSkVDVF9SRVNQT05TRT+uUmVqZWN0ZWRSZWFzb26vT1JERVJfTk9UX0ZPVU5E'
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderCancelReject))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual('REJECT_RESPONSE?', result.rejected_response_to.value)
        self.assertEqual('ORDER_NOT_FOUND', result.rejected_reason.value)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.rejected_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_modified_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'h6RUeXBlrU9yZGVyTW9kaWZpZWSiSWTZJDQ4Njc3MzYyLTYyZmItNDVkMS1iOGI5LWYwM2JmYTZjYzEwMalUaW1lc3RhbXC4MTk3MC0wMS0wMVQwMDowMDowMC4wMDBap09yZGVySWSoTy0xMjM0NTatT3JkZXJJZEJyb2tlcqlCTy0xMjM0NTatTW9kaWZpZWRQcmljZaEyrE1vZGlmaWVkVGltZbgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFo='
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

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
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'haRUeXBlrE9yZGVyRXhwaXJlZKJJZNkkY2Q1YjE0NjMtOWUyZS00N2E2LWE4NzQtYmI5Mjc5YzdhNGQyqVRpbWVzdGFtcLgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFqnT3JkZXJJZKhPLTEyMzQ1NqtFeHBpcmVkVGltZbgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFo='
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderExpired))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.expired_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, timezone.utc), result.timestamp)

    def test_can_deserialize_order_partially_filled_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'jKRUeXBltE9yZGVyUGFydGlhbGx5RmlsbGVkoklk2SQ3NzgzYTI2MC1mZjI1LTQ1ODItYTVlZC02YjYxMTYwZGFmYjCpVGltZXN0YW1wuDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWqdPcmRlcklkqE8tMTIzNDU2q0V4ZWN1dGlvbklkp0UxMjM0NTavRXhlY3V0aW9uVGlja2V0p1AxMjM0NTamU3ltYm9sq0FVRFVTRC5GWENNqU9yZGVyU2lkZaNCVVmuRmlsbGVkUXVhbnRpdHnSAADDUK5MZWF2ZXNRdWFudGl0edIAAMNQrEF2ZXJhZ2VQcmljZaMyLjCtRXhlY3V0aW9uVGltZbgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFo='
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderPartiallyFilled))
        self.assertEqual(OrderId('O-123456'), result.order_id)
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
        base64 = 'i6RUeXBlq09yZGVyRmlsbGVkoklk2SQ2ZjQxYjYxOS1kYWIzLTQ0M2UtODg2MS1mODVjMmNiODdjMjWpVGltZXN0YW1wuDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWqdPcmRlcklkqE8tMTIzNDU2q0V4ZWN1dGlvbklkp0UxMjM0NTavRXhlY3V0aW9uVGlja2V0p1AxMjM0NTamU3ltYm9sq0FVRFVTRC5GWENNqU9yZGVyU2lkZaNCVVmuRmlsbGVkUXVhbnRpdHnSAAGGoKxBdmVyYWdlUHJpY2WjMi4wrUV4ZWN1dGlvblRpbWW4MTk3MC0wMS0wMVQwMDowMDowMC4wMDBa'
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderFilled))
        self.assertEqual(OrderId('O-123456'), result.order_id)
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
