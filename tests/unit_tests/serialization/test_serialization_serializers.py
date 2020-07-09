# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import pytz
import uuid
import unittest
from datetime import datetime
from base64 import b64encode, b64decode

from nautilus_trader.core.decimal import Decimal
from nautilus_trader.core.types import ValidString, GUID, Label
from nautilus_trader.model.identifiers import IdTag, OrderId, OrderIdBroker, ExecutionId
from nautilus_trader.model.identifiers import PositionId, PositionIdBroker, StrategyId, AccountId
from nautilus_trader.model.enums import OrderSide, OrderType, OrderPurpose, TimeInForce, Currency
from nautilus_trader.model.enums import AccountType, SecurityType
from nautilus_trader.model.identifiers import Symbol, Venue
from nautilus_trader.model.objects import Price, Quantity, Instrument
from nautilus_trader.model.commands import AccountInquiry, SubmitOrder, SubmitBracketOrder
from nautilus_trader.model.commands import ModifyOrder, CancelOrder
from nautilus_trader.model.events import AccountStateEvent, OrderInitialized, OrderInvalid
from nautilus_trader.model.events import OrderDenied, OrderSubmitted, OrderAccepted, OrderRejected
from nautilus_trader.model.events import OrderWorking, OrderExpired, OrderModified, OrderCancelled
from nautilus_trader.model.events import OrderCancelReject, OrderPartiallyFilled, OrderFilled
from nautilus_trader.model.order import Order
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import LogMessage, LogLevel
from nautilus_trader.serialization.base import Serializer
from nautilus_trader.serialization.serializers import MsgPackDictionarySerializer
from nautilus_trader.serialization.serializers import MsgPackRequestSerializer
from nautilus_trader.serialization.serializers import MsgPackResponseSerializer
from nautilus_trader.serialization.serializers import MsgPackOrderSerializer
from nautilus_trader.serialization.serializers import MsgPackCommandSerializer
from nautilus_trader.serialization.serializers import MsgPackEventSerializer
from nautilus_trader.serialization.serializers import MsgPackLogSerializer
from nautilus_trader.serialization.data import BsonInstrumentSerializer
from nautilus_trader.network.identifiers import ClientId, ServerId, SessionId
from nautilus_trader.network.messages import Connect, Connected, Disconnect, Disconnected
from nautilus_trader.network.messages import DataRequest, DataResponse

from tests.test_kit.stubs import TestStubs, UNIX_EPOCH

AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()


class SerializerBaseTests(unittest.TestCase):
    def setUp(self):
        # Fixture Setup
        self.serializer = Serializer()

    def test_can_convert_camel_case_to_snake_case(self):
        # Arrange
        value0 = 'CamelCase'
        value1 = 'camelCase'
        value2 = 'camel'

        # Act
        result0 = self.serializer.py_convert_camel_to_snake(value0)
        result1 = self.serializer.py_convert_camel_to_snake(value1)
        result2 = self.serializer.py_convert_camel_to_snake(value2)

        # Assert
        self.assertEqual('CAMEL_CASE', result0)
        self.assertEqual('CAMEL_CASE', result1)
        self.assertEqual('CAMEL', result2)

    def test_can_convert_snake_case_to_camel_case(self):
        # Arrange
        value0 = 'SNAKE_CASE'
        value1 = 'snake_case'
        value2 = 'snake'

        # Act
        result0 = self.serializer.py_convert_snake_to_camel(value0)
        result1 = self.serializer.py_convert_snake_to_camel(value1)
        result2 = self.serializer.py_convert_snake_to_camel(value2)

        # Assert
        self.assertEqual('SnakeCase', result0)
        self.assertEqual('SnakeCase', result1)
        self.assertEqual('Snake', result2)


class MsgPackDictionarySerializerTests(unittest.TestCase):

    def test_can_serialize_and_deserialize_string_dictionaries(self):
        # Arrange
        data = {'A': '1', 'B': '2', 'C': '3'}
        serializer = MsgPackDictionarySerializer()

        # Act
        serialized = serializer.serialize(data)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(data, deserialized)


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
        print(b64encode(serialized))
        print(order)

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
            OrderId('O-123456'),
            AUDUSD_FXCM,
            OrderSide.BUY,
            OrderType.LIMIT,
            Quantity(100000),
            price=Price(1.00000, 5),
            label=None,
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH,
            init_id=GUID(uuid.uuid4()),
            timestamp=UNIX_EPOCH)

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
            OrderId('O-123456'),
            AUDUSD_FXCM,
            OrderSide.BUY,
            OrderType.STOP_LIMIT,
            Quantity(100000),
            price=Price(1.00000, 5),
            label=Label('S1_SL'),
            init_id=GUID(uuid.uuid4()),
            timestamp=UNIX_EPOCH)

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
            OrderId('O-123456'),
            AUDUSD_FXCM,
            OrderSide.BUY,
            OrderType.STOP_LIMIT,
            Quantity(100000),
            price=Price(1.00000, 5),
            label=None,
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH,
            init_id=GUID(uuid.uuid4()),
            timestamp=UNIX_EPOCH)

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
        self.trader_id = TestStubs.trader_id()
        self.account_id = TestStubs.account_id()
        self.serializer = MsgPackCommandSerializer()
        self.order_factory = OrderFactory(
            id_tag_trader=IdTag('001'),
            id_tag_strategy=IdTag('001'),
            clock=TestClock())
        print('\n')

    def test_can_serialize_and_deserialize_account_inquiry_command(self):
        # Arrange
        command = AccountInquiry(
            self.trader_id,
            self.account_id,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, command)
        print(b64encode(serialized))
        print(command)

    def test_can_deserialize_account_inquiry_command_from_csharp(self):
        # Arrange
        base64 = 'haRUeXBlxA5BY2NvdW50SW5xdWlyeaJJZMQkNjcxODYxMzQtZTI0Yy00NWZiLTk0NGUtNzNmMDUxZDMxMmIzqVRpbWVzdGFtcMQYMTk3MC0wMS0wMVQwMDowMDowMC4wMDBaqFRyYWRlcklkxApURVNURVItMDAwqUFjY291bnRJZMQYRlhDTS0wMjg5OTk5OTktU0lNVUxBVEVE'
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, AccountInquiry))

    def test_can_serialize_and_deserialize_submit_order_commands(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        command = SubmitOrder(
            self.trader_id,
            self.account_id,
            StrategyId('SCALPER', '01'),
            PositionId('P-123456'),
            order,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(command, deserialized)
        self.assertEqual(order, deserialized.order)
        print(command)
        print(len(serialized))
        print(serialized)
        print(b64encode(serialized))

    def test_can_deserialize_submit_order_commands_from_csharp(self):
        # Arrange
        base64 = 'iKRUeXBlxAtTdWJtaXRPcmRlcqJJZMQkMjJhMDY3NmQtMjUxMC00ZDE1LWIxYWEtMDI5ZmY2ZTI0YTBmqVRpbWVzdGFtcMQYMTk3MC0wMS0wMVQwMDowMDowMC4wMDBaqFRyYWRlcklkxApURVNURVItMDAwqlN0cmF0ZWd5SWTEDEVNQUNyb3NzLTAwMalBY2NvdW50SWTEGEZYQ00tMDI4OTk5OTk5LVNJTVVMQVRFRKpQb3NpdGlvbklkxAhQLTEyMzQ1NqVPcmRlcsT4jKJJZMQITy0xMjM0NTamU3ltYm9sxAtBVURVU0QuRlhDTaVMYWJlbMQKVEVTVF9PUkRFUqlPcmRlclNpZGXEA0J1ealPcmRlclR5cGXEBk1hcmtldKxPcmRlclB1cnBvc2XEBE5vbmWoUXVhbnRpdHnEBjEwMDAwMKVQcmljZcQETm9uZatUaW1lSW5Gb3JjZcQDREFZqkV4cGlyZVRpbWXEBE5vbmWpVGltZXN0YW1wxBgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFqmSW5pdElkxCQzMjgwNWYyOS1kYmE5LTRlMWUtOTBmMC04NDhjMzc2OGQzMWU='
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, SubmitOrder))

    def test_can_serialize_and_deserialize_submit_bracket_order_no_take_profit_commands(self):
        # Arrange
        bracket_order = self.order_factory.bracket_market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(0.99900, 5))

        command = SubmitBracketOrder(
            self.trader_id,
            self.account_id,
            StrategyId('SCALPER', '01'),
            PositionId('P-123456'),
            bracket_order,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(command, deserialized)
        self.assertEqual(bracket_order, deserialized.bracket_order)
        print(b64encode(serialized))
        print(command)

    def test_can_deserialize_submit_bracket_order_no_take_profit_from_csharp(self):
        # Arrange
        base64 = 'iqRUeXBlxBJTdWJtaXRCcmFja2V0T3JkZXKiSWTEJGJkOGM5YTZhLTNlNmItNGUzYS05OGYzLWEzMzRjZDM3NDkzNqlUaW1lc3RhbXDEGDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWqhUcmFkZXJJZMQKVEVTVEVSLTAwMKpTdHJhdGVneUlkxAxFTUFDcm9zcy0wMDGpQWNjb3VudElkxBhGWENNLTAyODk5OTk5OS1TSU1VTEFURUSqUG9zaXRpb25JZMQIUC0xMjM0NTalRW50cnnE+IyiSWTECE8tMTIzNDU2plN5bWJvbMQLQVVEVVNELkZYQ02lTGFiZWzEClRFU1RfT1JERVKpT3JkZXJTaWRlxANCdXmpT3JkZXJUeXBlxAZNYXJrZXSsT3JkZXJQdXJwb3NlxAROb25lqFF1YW50aXR5xAYxMDAwMDClUHJpY2XEBE5vbmWrVGltZUluRm9yY2XEA0RBWapFeHBpcmVUaW1lxAROb25lqVRpbWVzdGFtcMQYMTk3MC0wMS0wMVQwMDowMDowMC4wMDBapkluaXRJZMQkMmI4M2Y0YzYtOWQ0ZC00ZTUxLTk3ODQtY2YwYjhjZjYwNDZlqFN0b3BMb3NzxPOMoklkxAhPLTEyMzQ1NqZTeW1ib2zEC0FVRFVTRC5GWENNpUxhYmVsxApURVNUX09SREVSqU9yZGVyU2lkZcQDQnV5qU9yZGVyVHlwZcQEU3RvcKxPcmRlclB1cnBvc2XEBE5vbmWoUXVhbnRpdHnEBjEwMDAwMKVQcmljZcQBMatUaW1lSW5Gb3JjZcQDREFZqkV4cGlyZVRpbWXEBE5vbmWpVGltZXN0YW1wxBgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFqmSW5pdElkxCQ1NTI3MGJhMC02Yjg2LTRlMTItYTc4ZS05YzY5ZDE3ZTEwZWKqVGFrZVByb2ZpdMQBgA=='
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, SubmitBracketOrder))

    def test_can_serialize_and_deserialize_submit_bracket_order_with_take_profit_commands(self):
        # Arrange
        bracket_order = self.order_factory.bracket_limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(0.99900, 5),
            Price(1.00000, 5),
            Price(1.00010, 5))

        command = SubmitBracketOrder(
            self.trader_id,
            self.account_id,
            StrategyId('SCALPER', '01'),
            PositionId('P-123456'),
            bracket_order,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(command, deserialized)
        self.assertEqual(bracket_order, deserialized.bracket_order)
        print(b64encode(serialized))
        print(command)

    def test_can_serialize_and_deserialize_modify_order_commands(self):
        # Arrange
        command = ModifyOrder(
            self.trader_id,
            self.account_id,
            OrderId('O-123456'),
            Quantity(100000),
            Price(1.00001, 5),
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(command, deserialized)
        print(b64encode(serialized))
        print(command)

    def test_can_serialize_and_deserialize_cancel_order_commands(self):
        # Arrange
        command = CancelOrder(
            self.trader_id,
            self.account_id,
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


class MsgPackEventSerializerTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.account_id = TestStubs.account_id()
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
            Price(1.50000, 5),
            OrderPurpose.NONE,
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
            self.account_id,
            OrderId('O-123456'),
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialize_and_deserialize_order_invalid_events(self):
        # Arrange
        event = OrderInvalid(
            OrderId('O-123456'),
            "OrderId already exists",
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_serialize_and_deserialize_order_denied_events(self):
        # Arrange
        event = OrderDenied(
            OrderId('O-123456'),
            "Exceeds risk for FX",
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
            self.account_id,
            OrderId('O-123456'),
            OrderIdBroker('B-123456'),
            Label('E'),
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
            self.account_id,
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
            self.account_id,
            OrderId('O-123456'),
            OrderIdBroker('B-123456'),
            AUDUSD_FXCM,
            Label('PT'),
            OrderSide.SELL,
            OrderType.STOP_LIMIT,
            Quantity(100000),
            Price(1.50000, 5),
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
            self.account_id,
            OrderId('O-123456'),
            OrderIdBroker('BO-123456'),
            AUDUSD_FXCM,
            Label('PT'),
            OrderSide.SELL,
            OrderType.STOP_LIMIT,
            Quantity(100000),
            Price(1.50000, 5),
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
            self.account_id,
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
            self.account_id,
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
            self.account_id,
            OrderId('O-123456'),
            OrderIdBroker('BO-123456'),
            Quantity(100000),
            Price(0.80010, 5),
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
            self.account_id,
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
            self.account_id,
            OrderId('O-123456'),
            ExecutionId('E123456'),
            PositionIdBroker('T123456'),
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(50000),
            Quantity(50000),
            Price(1.00000, 5),
            Currency.USD,
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
            self.account_id,
            OrderId('O-123456'),
            ExecutionId('E123456'),
            PositionIdBroker('T123456'),
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000),
            Price(1.00000, 5),
            Currency.USD,
            UNIX_EPOCH,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_can_deserialize_account_state_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'jKRUeXBlsUFjY291bnRTdGF0ZUV2ZW50oklk2SQyYTk4NjM5ZC1lMzJkLTQxMDctYmRmMC1hYTU2ODUwMjFiOGOpVGltZXN0YW1wuDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWqlBY2NvdW50SWS2RlhDTS1EMTIzNDU2LVNJTVVMQVRFRKhDdXJyZW5jeaNVU0SrQ2FzaEJhbGFuY2WmMTAwMDAwrENhc2hTdGFydERheaYxMDAwMDCvQ2FzaEFjdGl2aXR5RGF5oTC1TWFyZ2luVXNlZExpcXVpZGF0aW9uoTC1TWFyZ2luVXNlZE1haW50ZW5hbmNloTCrTWFyZ2luUmF0aW+hMLBNYXJnaW5DYWxsU3RhdHVzoU4='
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, AccountStateEvent))
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.timestamp)

    def test_can_deserialize_order_invalid_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'haRUeXBlrE9yZGVySW52YWxpZKJJZNkkNzE0N2UyOTktYjkxNC00ZTE0LTgyYzItN2I0ZmU5MDMwZThiqVRpbWVzdGFtcLgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFqnT3JkZXJJZKhPLTEyMzQ1Nq1JbnZhbGlkUmVhc29ut09yZGVySWQgYWxyZWFkeSBleGlzdHMu'
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderInvalid))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual('OrderId already exists.', result.invalid_reason)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.timestamp)

    def test_can_deserialize_order_denied_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'haRUeXBlq09yZGVyRGVuaWVkoklk2SQ1ZTgyNzllNC02NGY1LTRhNTAtYjBiYy1iYzI0NzAyMjlkMTGpVGltZXN0YW1wuDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWqdPcmRlcklkqE8tMTIzNDU2rERlbmllZFJlYXNvbrRFeGNlZWRzIHJpc2sgZm9yIEZYLg=='
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderDenied))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual('Exceeds risk for FX.', result.denied_reason)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.timestamp)

    def test_can_deserialize_order_submitted_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'hqRUeXBlrk9yZGVyU3VibWl0dGVkoklk2SQxMThhZjIyZC1jMGQwLTQwNDEtOWQzMS0xYjI4ZWJiYmEzMjCpVGltZXN0YW1wuDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWqdPcmRlcklkqE8tMTIzNDU2qUFjY291bnRJZLJGWENNLTAyODUxOTA4LURFTU+tU3VibWl0dGVkVGltZbgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFo='
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderSubmitted))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(AccountId('FXCM', '02851908', AccountType.DEMO), result.account_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.submitted_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.timestamp)

    def test_can_deserialize_order_accepted_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'iKRUeXBlrU9yZGVyQWNjZXB0ZWSiSWTZJDIzMWFiNjc2LWM4NzItNGJkNC04NmNkLTAwYWMzOGM1Zjc2MqlUaW1lc3RhbXC4MTk3MC0wMS0wMVQwMDowMDowMC4wMDBaqUFjY291bnRJZLJGWENNLTAyODUxOTA4LURFTU+nT3JkZXJJZKhPLTEyMzQ1Nq1PcmRlcklkQnJva2VyqUJPLTEyMzQ1NqVMYWJlbKpURVNUX09SREVSrEFjY2VwdGVkVGltZbgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFo='
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderAccepted))
        self.assertEqual(AccountId('FXCM', '02851908', AccountType.DEMO), result.account_id)
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(OrderIdBroker('BO-123456'), result.order_id_broker)
        self.assertEqual(Label('TEST_ORDER'), result.label)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.accepted_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.timestamp)

    def test_can_deserialize_order_rejected_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'h6RUeXBlrU9yZGVyUmVqZWN0ZWSiSWTZJDFkMWM2NTRmLTQ2MTQtNDFlZC1iNDBlLWU0YzRlMzc3MmQ2NqlUaW1lc3RhbXC4MTk3MC0wMS0wMVQwMDowMDowMC4wMDBap09yZGVySWSoTy0xMjM0NTapQWNjb3VudElkskZYQ00tMDI4NTE5MDgtREVNT6xSZWplY3RlZFRpbWW4MTk3MC0wMS0wMVQwMDowMDowMC4wMDBarlJlamVjdGVkUmVhc29urUlOVkFMSURfT1JERVI='
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderRejected))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.rejected_time)
        self.assertEqual('INVALID_ORDER', result.rejected_reason.value)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.timestamp)

    def test_can_deserialize_order_working_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'j6RUeXBlrE9yZGVyV29ya2luZ6JJZNkkYzUxNzIzMGItMjM3MC00ZTE2LTg5YTUtZjA2ZTg1YThmZmU1qVRpbWVzdGFtcLgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFqpQWNjb3VudElkskZYQ00tMDI4NTE5MDgtREVNT6dPcmRlcklkqE8tMTIzNDU2rU9yZGVySWRCcm9rZXKpQk8tMTIzNDU2plN5bWJvbKtBVURVU0QuRlhDTaVMYWJlbKFFqU9yZGVyU2lkZaNCdXmpT3JkZXJUeXBlpFN0b3CoUXVhbnRpdHmmMTAwMDAwpVByaWNloTGrVGltZUluRm9yY2WjREFZqkV4cGlyZVRpbWWkTm9uZatXb3JraW5nVGltZbgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFo='
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderWorking))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(OrderIdBroker('BO-123456'), result.order_id_broker)
        self.assertEqual(AccountId('FXCM', '02851908', AccountType.DEMO), result.account_id)
        self.assertEqual(Symbol('AUDUSD', Venue('FXCM')), result.symbol)
        self.assertEqual(Label('E'), result.label)
        self.assertEqual(OrderType.STOP, result.order_type)
        self.assertEqual(Quantity(100000), result.quantity)
        self.assertEqual(Price(1, 1), result.price)
        self.assertEqual(TimeInForce.DAY, result.time_in_force)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.working_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.timestamp)
        self.assertIsNone(result.expire_time)

    def test_can_deserialize_order_working_events_with_expire_time_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'j6RUeXBlrE9yZGVyV29ya2luZ6JJZNkkZGMyNjVmZjAtMWY1Ny00ZmUzLWJiY2UtMDZmN2M3YjQ2MjA4qVRpbWVzdGFtcLgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFqpQWNjb3VudElkskZYQ00tMDI4NTE5MDgtREVNT6dPcmRlcklkqE8tMTIzNDU2rU9yZGVySWRCcm9rZXKpQk8tMTIzNDU2plN5bWJvbKtBVURVU0QuRlhDTaVMYWJlbKFFqU9yZGVyU2lkZaNCdXmpT3JkZXJUeXBlpFN0b3CoUXVhbnRpdHmmMTAwMDAwpVByaWNloTGrVGltZUluRm9yY2WjR1REqkV4cGlyZVRpbWW4MTk3MC0wMS0wMVQwMDowMTowMC4wMDBaq1dvcmtpbmdUaW1luDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWg=='
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderWorking))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(OrderIdBroker('BO-123456'), result.order_id_broker)
        self.assertEqual(AccountId('FXCM', '02851908', AccountType.DEMO), result.account_id)
        self.assertEqual(Symbol('AUDUSD', Venue('FXCM')), result.symbol)
        self.assertEqual(Label('E'), result.label)
        self.assertEqual(OrderSide.BUY, result.order_side)
        self.assertEqual(OrderType.STOP, result.order_type)
        self.assertEqual(Quantity(100000), result.quantity)
        self.assertEqual(Price(1, 1), result.price)
        self.assertEqual(TimeInForce.GTD, result.time_in_force)
        self.assertEqual(datetime(1970, 1, 1, 0, 0, 0, 0, pytz.utc), result.working_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 0, 0, 0, 0, pytz.utc), result.timestamp)
        self.assertEqual(datetime(1970, 1, 1, 0, 1, 0, 0, pytz.utc), result.expire_time)

    def test_can_deserialize_order_cancelled_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'hqRUeXBlrk9yZGVyQ2FuY2VsbGVkoklk2SQ0M2EwY2RiNC03YTUyLTRjYWQtYjEyMy04MGZiYmYxNDM3MDmpVGltZXN0YW1wuDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWqdPcmRlcklkqE8tMTIzNDU2qUFjY291bnRJZLJGWENNLTAyODUxOTA4LURFTU+tQ2FuY2VsbGVkVGltZbgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFo='
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderCancelled))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(AccountId('FXCM', '02851908', AccountType.DEMO), result.account_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.cancelled_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.timestamp)

    def test_can_deserialize_order_cancel_reject_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'iKRUeXBlsU9yZGVyQ2FuY2VsUmVqZWN0oklk2SQ5YTFlYzgyZi04NDZkLTQ3YzctODJlOS1lYzIwNGQ4MzFmOWKpVGltZXN0YW1wuDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWqdPcmRlcklkqE8tMTIzNDU2qUFjY291bnRJZLJGWENNLTAyODUxOTA4LURFTU+sUmVqZWN0ZWRUaW1luDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWrJSZWplY3RlZFJlc3BvbnNlVG+wUkVKRUNUX1JFU1BPTlNFP65SZWplY3RlZFJlYXNvbq9PUkRFUl9OT1RfRk9VTkQ='
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderCancelReject))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(AccountId('FXCM', '02851908', AccountType.DEMO), result.account_id)
        self.assertEqual('REJECT_RESPONSE?', result.rejected_response_to.value)
        self.assertEqual('ORDER_NOT_FOUND', result.rejected_reason.value)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.rejected_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.timestamp)

    def test_can_deserialize_order_modified_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'iaRUeXBlrU9yZGVyTW9kaWZpZWSiSWTZJDFkOGFlMDNkLWExMzYtNDM5ZC05ZmRlLTYwNDAzYTU1ZWMzOKlUaW1lc3RhbXC4MTk3MC0wMS0wMVQwMDowMDowMC4wMDBaqUFjY291bnRJZLJGWENNLTAyODUxOTA4LURFTU+nT3JkZXJJZKhPLTEyMzQ1Nq1PcmRlcklkQnJva2VyqUJPLTEyMzQ1NrBNb2RpZmllZFF1YW50aXR5pjEwMDAwMK1Nb2RpZmllZFByaWNloTKsTW9kaWZpZWRUaW1luDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWg=='
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderModified))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(OrderIdBroker('BO-123456'), result.order_id_broker)
        self.assertEqual(AccountId('FXCM', '02851908', AccountType.DEMO), result.account_id)
        self.assertEqual(Price(2, 1), result.modified_price)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.modified_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.timestamp)

    def test_can_deserialize_order_expired_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'hqRUeXBlrE9yZGVyRXhwaXJlZKJJZNkkY2EwOTQ5YTEtNmM0MC00NzVmLWEwNzQtM2JiYzUzYTI5Y2JkqVRpbWVzdGFtcLgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFqnT3JkZXJJZKhPLTEyMzQ1NqlBY2NvdW50SWSyRlhDTS0wMjg1MTkwOC1ERU1Pq0V4cGlyZWRUaW1luDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWg=='
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderExpired))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(AccountId('FXCM', '02851908', AccountType.DEMO), result.account_id)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.expired_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.timestamp)

    def test_can_deserialize_order_partially_filled_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'jqRUeXBltE9yZGVyUGFydGlhbGx5RmlsbGVkoklk2SQwOTk3Nzk1Ny0zMzE3LTQ3ODgtOGYxOC1lMmEyY2I0ZDljYmSpVGltZXN0YW1wuDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWqlBY2NvdW50SWSyRlhDTS0wMjg1MTkwOC1ERU1Pp09yZGVySWSoTy0xMjM0NTarRXhlY3V0aW9uSWSnRTEyMzQ1NrBQb3NpdGlvbklkQnJva2Vyp1AxMjM0NTamU3ltYm9sq0FVRFVTRC5GWENNqU9yZGVyU2lkZaNCdXmuRmlsbGVkUXVhbnRpdHmlNTAwMDCuTGVhdmVzUXVhbnRpdHmlNTAwMDCsQXZlcmFnZVByaWNlozIuMKhDdXJyZW5jeaNVU0StRXhlY3V0aW9uVGltZbgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFo='
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderPartiallyFilled))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(AccountId('FXCM', '02851908', AccountType.DEMO), result.account_id)
        self.assertEqual(ExecutionId('E123456'), result.execution_id)
        self.assertEqual(PositionIdBroker('P123456'), result.position_id_broker)
        self.assertEqual(Symbol('AUDUSD', Venue('FXCM')), result.symbol)
        self.assertEqual(840, result.transaction_currency)
        self.assertEqual(OrderSide.BUY, result.order_side)
        self.assertEqual(Quantity(50000), result.filled_quantity)
        self.assertEqual(Quantity(50000), result.leaves_quantity)
        self.assertEqual(Price(2, 1), result.average_price)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.execution_time)
        self.assertTrue(isinstance(result.id, GUID))
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.timestamp)

    def test_can_deserialize_order_filled_events_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'jaRUeXBlq09yZGVyRmlsbGVkoklk2SRjYTg4NWZhZi1hNjE3LTQ3ZjUtYTUyZi0yNjljZGFlZmU4NDepVGltZXN0YW1wuDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWqlBY2NvdW50SWSyRlhDTS0wMjg1MTkwOC1ERU1Pp09yZGVySWSoTy0xMjM0NTarRXhlY3V0aW9uSWSnRTEyMzQ1NrBQb3NpdGlvbklkQnJva2Vyp1AxMjM0NTamU3ltYm9sq0FVRFVTRC5GWENNqU9yZGVyU2lkZaNCdXmuRmlsbGVkUXVhbnRpdHmmMTAwMDAwrEF2ZXJhZ2VQcmljZaMyLjCoQ3VycmVuY3mjVVNErUV4ZWN1dGlvblRpbWW4MTk3MC0wMS0wMVQwMDowMDowMC4wMDBa'
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, OrderFilled))
        self.assertEqual(OrderId('O-123456'), result.order_id)
        self.assertEqual(AccountId('FXCM', '02851908', AccountType.DEMO), result.account_id)
        self.assertEqual(ExecutionId('E123456'), result.execution_id)
        self.assertEqual(PositionIdBroker('P123456'), result.position_id_broker)
        self.assertEqual(Symbol('AUDUSD', Venue('FXCM')), result.symbol)
        self.assertEqual(840, result.transaction_currency)
        self.assertEqual(OrderSide.BUY, result.order_side)
        self.assertEqual(Quantity(100000), result.filled_quantity)
        self.assertEqual(Price(2, 1), result.average_price)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.execution_time)
        self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.timestamp)


class MsgPackInstrumentSerializerTests(unittest.TestCase):

    def test_can_serialize_and_deserialize_instrument(self):
        # Arrange
        serializer = BsonInstrumentSerializer()

        instrument = Instrument(
            symbol=Symbol('AUDUSD', Venue('FXCM')),
            broker_symbol='AUD/USD',
            quote_currency=Currency.USD,
            security_type=SecurityType.FOREX,
            price_precision=5,
            size_precision=0,
            tick_size=Price(0.00001, 5),
            round_lot_size=Quantity(1000),
            min_stop_distance_entry=0,
            min_stop_distance=0,
            min_limit_distance_entry=1,
            min_limit_distance=1,
            min_trade_size=Quantity(1),
            max_trade_size=Quantity(50000000),
            rollover_interest_buy=Decimal(0.025, 3),
            rollover_interest_sell=Decimal(-0.035, 3),
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
        self.assertEqual(instrument.price_precision, deserialized.price_precision)
        self.assertEqual(instrument.size_precision, deserialized.size_precision)
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

    def test_can_deserialize_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = 'vgEAAAJTeW1ib2wADAAAAEFVRFVTRC5GWENNAAJCcm9rZXJTeW1ib2wACAAAAEFVRC9VU0QAAlF1b3RlQ3VycmVuY3kABAAAAFVTRAACU2VjdXJpdHlUeXBlAAYAAABGb3JleAAQUHJpY2VQcmVjaXNpb24ABQAAABBTaXplUHJlY2lzaW9uAAAAAAAQTWluU3RvcERpc3RhbmNlRW50cnkAAAAAABBNaW5TdG9wRGlzdGFuY2UAAAAAABBNaW5MaW1pdERpc3RhbmNlRW50cnkAAAAAABBNaW5MaW1pdERpc3RhbmNlAAAAAAACVGlja1NpemUACAAAADAuMDAwMDEAAlJvdW5kTG90U2l6ZQAFAAAAMTAwMAACTWluVHJhZGVTaXplAAIAAAAxAAJNYXhUcmFkZVNpemUACQAAADUwMDAwMDAwAAJSb2xsb3ZlckludGVyZXN0QnV5AAIAAAAxAAJSb2xsb3ZlckludGVyZXN0U2VsbAACAAAAMQACVGltZXN0YW1wABkAAAAxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFoAAkJhc2VDdXJyZW5jeQAEAAAAQVVEAAA='
        body = b64decode(base64)

        # Act
        serializer = BsonInstrumentSerializer()
        deserialized = serializer.deserialize(body)
        print(deserialized)
        self.assertTrue(isinstance(deserialized, Instrument))


class MsgPackRequestSerializerTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.serializer = MsgPackRequestSerializer()

    def test_can_serialize_and_deserialize_connect_requests(self):
        # Arrange
        client_id = ClientId("Trader-001")
        timestamp = UNIX_EPOCH
        authentication = SessionId.py_create(client_id, timestamp, 'None')

        request = Connect(
            ClientId("Trader-001"),
            authentication.value,
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(request)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertTrue(isinstance(deserialized, Connect))
        self.assertEqual("Trader-001", deserialized.client_id.value)
        self.assertEqual("e5db3dad8222a27e5d2991d11ad65f0f74668a4cfb629e97aa6920a73a012f87", deserialized.authentication)

    def test_can_serialize_and_deserialize_disconnect_requests(self):
        # Arrange
        request = Disconnect(
            ClientId("Trader-001"),
            SessionId("Trader-001-1970-1-1-0"),
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(request)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertTrue(isinstance(deserialized, Disconnect))
        self.assertEqual("Trader-001", deserialized.client_id.value)
        self.assertEqual("Trader-001-1970-1-1-0", deserialized.session_id.value)

    def test_can_serialize_and_deserialize_tick_data_requests(self):
        # Arrange
        query = {
            "DataType": "Tick[]",
            "Symbol": "AUDUSD.FXCM",
            "FromDate": str(UNIX_EPOCH.date()),
            "ToDate": str(UNIX_EPOCH.date()),
            "Limit": "0",
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
            "Specification": "1-MIN-BID",
            "FromDate": str(UNIX_EPOCH.date()),
            "ToDate": str(UNIX_EPOCH.date()),
            "Limit": "0",
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

    def test_can_serialize_and_deserialize_connected_responses(self):
        # Arrange
        request = Connected(
            "Trader-001 connected to session",
            ServerId("NautilusData.CommandServer"),
            SessionId("3c95b0db407d8b28827d9f2a23cd54048956a35ab1441a54ebd43b2aedf282ea"),
            GUID(uuid.uuid4()),
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(request)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertTrue(isinstance(deserialized, Connected))
        self.assertEqual("Trader-001 connected to session", deserialized.message)
        self.assertEqual("NautilusData.CommandServer", deserialized.server_id.value)
        self.assertEqual("3c95b0db407d8b28827d9f2a23cd54048956a35ab1441a54ebd43b2aedf282ea", deserialized.session_id.value)

    def test_can_serialize_and_deserialize_disconnected_responses(self):
        # Arrange
        request = Disconnected(
            "Trader-001 disconnected from session",
            ServerId("NautilusData.CommandServer"),
            SessionId("3c95b0db407d8b28827d9f2a23cd54048956a35ab1441a54ebd43b2aedf282ea"),
            GUID(uuid.uuid4()),
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(request)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertTrue(isinstance(deserialized, Disconnected))
        self.assertEqual("Trader-001 disconnected from session", deserialized.message)
        self.assertEqual("NautilusData.CommandServer", deserialized.server_id.value)
        self.assertEqual("3c95b0db407d8b28827d9f2a23cd54048956a35ab1441a54ebd43b2aedf282ea", deserialized.session_id.value)

    def test_can_serialize_and_deserialize_data_responses(self):
        # Arrange
        data = b'\x01 \x00'
        data_type = 'NothingUseful'
        data_encoding = 'BSON'

        response = DataResponse(
            data=data,
            data_type='NothingUseful',
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
        self.assertEqual(data_type, deserialized.data_type)
        self.assertEqual(data_encoding, deserialized.data_encoding)

        print(deserialized)


class MsgPackLogSerializerTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.serializer = MsgPackLogSerializer()

    def test_can_serialize_and_deserialize_log_messages(self):
        # Arrange
        message = LogMessage(
            timestamp=UNIX_EPOCH,
            level=LogLevel.DEBUG,
            text="This is a test message")

        # Act
        serialized = self.serializer.serialize(message)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertTrue(isinstance(deserialized, LogMessage))
        print(deserialized)
