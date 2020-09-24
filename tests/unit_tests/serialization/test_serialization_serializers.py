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

from base64 import b64decode
from base64 import b64encode
import unittest

from nautilus_trader.backtest.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.logging import LogLevel
from nautilus_trader.common.logging import LogMessage
from nautilus_trader.core.decimal import Decimal64
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.model.commands import AccountInquiry
from nautilus_trader.model.commands import CancelOrder
from nautilus_trader.model.commands import ModifyOrder
from nautilus_trader.model.commands import SubmitBracketOrder
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.enums import Currency
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import SecurityType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events import OrderAccepted
from nautilus_trader.model.events import OrderCancelReject
from nautilus_trader.model.events import OrderCancelled
from nautilus_trader.model.events import OrderDenied
from nautilus_trader.model.events import OrderExpired
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderInitialized
from nautilus_trader.model.events import OrderInvalid
from nautilus_trader.model.events import OrderModified
from nautilus_trader.model.events import OrderPartiallyFilled
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.events import OrderSubmitted
from nautilus_trader.model.events import OrderWorking
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ClientPositionId
from nautilus_trader.model.identifiers import ExecutionId
from nautilus_trader.model.identifiers import IdTag
from nautilus_trader.model.identifiers import OrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instrument import Instrument
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.order import LimitOrder
from nautilus_trader.model.order import StopOrder
from nautilus_trader.network.identifiers import ClientId
from nautilus_trader.network.identifiers import ServerId
from nautilus_trader.network.identifiers import SessionId
from nautilus_trader.network.messages import Connect
from nautilus_trader.network.messages import Connected
from nautilus_trader.network.messages import DataRequest
from nautilus_trader.network.messages import DataResponse
from nautilus_trader.network.messages import Disconnect
from nautilus_trader.network.messages import Disconnected
from nautilus_trader.serialization.base import Serializer
from nautilus_trader.serialization.data import BsonInstrumentSerializer
from nautilus_trader.serialization.serializers import MsgPackCommandSerializer
from nautilus_trader.serialization.serializers import MsgPackDictionarySerializer
from nautilus_trader.serialization.serializers import MsgPackEventSerializer
from nautilus_trader.serialization.serializers import MsgPackLogSerializer
from nautilus_trader.serialization.serializers import MsgPackOrderSerializer
from nautilus_trader.serialization.serializers import MsgPackRequestSerializer
from nautilus_trader.serialization.serializers import MsgPackResponseSerializer
from tests.test_kit.stubs import TestStubs
from tests.test_kit.stubs import UNIX_EPOCH

AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()


class SerializerBaseTests(unittest.TestCase):
    def setUp(self):
        # Fixture Setup
        self.serializer = Serializer()

    def test_py_convert_camel_to_snake(self):
        # Arrange
        value0 = "CamelCase"
        value1 = "camelCase"
        value2 = "camel"

        # Act
        result0 = self.serializer.py_convert_camel_to_snake(value0)
        result1 = self.serializer.py_convert_camel_to_snake(value1)
        result2 = self.serializer.py_convert_camel_to_snake(value2)

        # Assert
        self.assertEqual("CAMEL_CASE", result0)
        self.assertEqual("CAMEL_CASE", result1)
        self.assertEqual("CAMEL", result2)

    def test_py_convert_snake_to_camel(self):
        # Arrange
        value0 = "SNAKE_CASE"
        value1 = "snake_case"
        value2 = "snake"

        # Act
        result0 = self.serializer.py_convert_snake_to_camel(value0)
        result1 = self.serializer.py_convert_snake_to_camel(value1)
        result2 = self.serializer.py_convert_snake_to_camel(value2)

        # Assert
        self.assertEqual("SnakeCase", result0)
        self.assertEqual("SnakeCase", result1)
        self.assertEqual("Snake", result2)


class MsgPackDictionarySerializerTests(unittest.TestCase):

    def test_serialize_and_deserialize_string_dictionaries(self):
        # Arrange
        data = {"A": "1", "B": "2", "C": "3"}
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
            id_tag_trader=IdTag("001"),
            id_tag_strategy=IdTag("001"),
            clock=TestClock())
        print("\n")

    def test_serialize_and_deserialize_market_orders(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        # Act
        serialized = self.serializer.serialize(order)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(order, deserialized)
        print(b64encode(serialized))
        print(order)

    def test_serialize_and_deserialize_limit_orders(self):
        # Arrange
        order = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(1.00000, 5),
            TimeInForce.DAY,
            expire_time=None)

        # Act
        serialized = self.serializer.serialize(order)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(order, deserialized)
        print(b64encode(serialized))
        print(order)

    def test_serialize_and_deserialize_limit_orders_with_expire_time(self):
        # Arrange
        order = LimitOrder(
            ClientOrderId("O-123456"),
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            price=Price(1.00000, 5),
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH,
            init_id=uuid4(),
            timestamp=UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(order)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(order, deserialized)
        print(b64encode(serialized))
        print(order)

    def test_serialize_and_deserialize_stop_orders_with_expire_time(self):
        # Arrange
        order = StopOrder(
            ClientOrderId("O-123456"),
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            price=Price(1.00000, 5),
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH,
            init_id=uuid4(),
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
            id_tag_trader=IdTag("001"),
            id_tag_strategy=IdTag("001"),
            clock=TestClock())
        print("\n")

    def test_serialize_and_deserialize_account_inquiry_command(self):
        # Arrange
        command = AccountInquiry(
            self.trader_id,
            self.account_id,
            uuid4(),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, command)
        print(b64encode(serialized))
        print(command)

    def test_deserialize_account_inquiry_command_from_csharp(self):
        # Arrange
        base64 = "haRUeXBlxA5BY2NvdW50SW5xdWlyeaJJZMQkNjcxODYxMzQtZTI0Yy00NWZ" \
                 "iLTk0NGUtNzNmMDUxZDMxMmIzqVRpbWVzdGFtcMQYMTk3MC0wMS0wMVQwMD" \
                 "owMDowMC4wMDBaqFRyYWRlcklkxApURVNURVItMDAwqUFjY291bnRJZMQYR" \
                 "lhDTS0wMjg5OTk5OTktU0lNVUxBVEVE"
        body = b64decode(base64)

        # Act
        result = self.serializer.deserialize(body)

        # Assert
        self.assertTrue(isinstance(result, AccountInquiry))

    def test_serialize_and_deserialize_submit_order_commands(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        command = SubmitOrder(
            self.trader_id,
            self.account_id,
            StrategyId("SCALPER", "01"),
            ClientPositionId("P-123456"),
            order,
            uuid4(),
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

    # TODO: Breaking changes to C# side
    # def test_deserialize_submit_order_commands_from_csharp(self):
    #     # Arrange
    #     base64 = "iKRUeXBlxAtTdWJtaXRPcmRlcqJJZMQkMjJhMDY3NmQtMjUxMC00ZDE1LWI" \
    #              "xYWEtMDI5ZmY2ZTI0YTBmqVRpbWVzdGFtcMQYMTk3MC0wMS0wMVQwMDowMD" \
    #              "owMC4wMDBaqFRyYWRlcklkxApURVNURVItMDAwqlN0cmF0ZWd5SWTEDEVNQ" \
    #              "UNyb3NzLTAwMalBY2NvdW50SWTEGEZYQ00tMDI4OTk5OTk5LVNJTVVMQVRF" \
    #              "RKpQb3NpdGlvbklkxAhQLTEyMzQ1NqVPcmRlcsT4jKJJZMQITy0xMjM0NTa" \
    #              "mU3ltYm9sxAtBVURVU0QuRlhDTaVMYWJlbMQKVEVTVF9PUkRFUqlPcmRlcl" \
    #              "NpZGXEA0J1ealPcmRlclR5cGXEBk1hcmtldKxPcmRlclB1cnBvc2XEBE5vb" \
    #              "mWoUXVhbnRpdHnEBjEwMDAwMKVQcmljZcQETm9uZatUaW1lSW5Gb3JjZcQD" \
    #              "REFZqkV4cGlyZVRpbWXEBE5vbmWpVGltZXN0YW1wxBgxOTcwLTAxLTAxVDA" \
    #              "wOjAwOjAwLjAwMFqmSW5pdElkxCQzMjgwNWYyOS1kYmE5LTRlMWUtOTBmMC" \
    #              "04NDhjMzc2OGQzMWU="
    #
    #     body = b64decode(base64)
    #
    #     # Act
    #     result = self.serializer.deserialize(body)
    #
    #     # Assert
    #     self.assertTrue(isinstance(result, SubmitOrder))

    def test_serialize_and_deserialize_submit_bracket_order_no_take_profit_commands(self):
        # Arrange
        entry_order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        bracket_order = self.order_factory.bracket(
            entry_order,
            stop_loss=Price(0.99900, 5))

        command = SubmitBracketOrder(
            self.trader_id,
            self.account_id,
            StrategyId("SCALPER", "01"),
            ClientPositionId("P-123456"),
            bracket_order,
            uuid4(),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(command, deserialized)
        self.assertEqual(bracket_order, deserialized.bracket_order)
        print(b64encode(serialized))
        print(command)

    # TODO: Breaking changes to C# side
    # def test_deserialize_submit_bracket_order_no_take_profit_from_csharp(self):
    #     # Arrange
    #     base64 = "iqRUeXBlxBJTdWJtaXRCcmFja2V0T3JkZXKiSWTEJGJkOGM5YTZhLTNlNmI" \
    #              "tNGUzYS05OGYzLWEzMzRjZDM3NDkzNqlUaW1lc3RhbXDEGDE5NzAtMDEtMD" \
    #              "FUMDA6MDA6MDAuMDAwWqhUcmFkZXJJZMQKVEVTVEVSLTAwMKpTdHJhdGVne" \
    #              "UlkxAxFTUFDcm9zcy0wMDGpQWNjb3VudElkxBhGWENNLTAyODk5OTk5OS1T" \
    #              "SU1VTEFURUSqUG9zaXRpb25JZMQIUC0xMjM0NTalRW50cnnE+IyiSWTECE8" \
    #              "tMTIzNDU2plN5bWJvbMQLQVVEVVNELkZYQ02lTGFiZWzEClRFU1RfT1JERV" \
    #              "KpT3JkZXJTaWRlxANCdXmpT3JkZXJUeXBlxAZNYXJrZXSsT3JkZXJQdXJwb" \
    #              "3NlxAROb25lqFF1YW50aXR5xAYxMDAwMDClUHJpY2XEBE5vbmWrVGltZUlu" \
    #              "Rm9yY2XEA0RBWapFeHBpcmVUaW1lxAROb25lqVRpbWVzdGFtcMQYMTk3MC0" \
    #              "wMS0wMVQwMDowMDowMC4wMDBapkluaXRJZMQkMmI4M2Y0YzYtOWQ0ZC00ZT" \
    #              "UxLTk3ODQtY2YwYjhjZjYwNDZlqFN0b3BMb3NzxPOMoklkxAhPLTEyMzQ1N" \
    #              "qZTeW1ib2zEC0FVRFVTRC5GWENNpUxhYmVsxApURVNUX09SREVSqU9yZGVy" \
    #              "U2lkZcQDQnV5qU9yZGVyVHlwZcQEU3RvcKxPcmRlclB1cnBvc2XEBE5vbmW" \
    #              "oUXVhbnRpdHnEBjEwMDAwMKVQcmljZcQBMatUaW1lSW5Gb3JjZcQDREFZqk" \
    #              "V4cGlyZVRpbWXEBE5vbmWpVGltZXN0YW1wxBgxOTcwLTAxLTAxVDAwOjAwO" \
    #              "jAwLjAwMFqmSW5pdElkxCQ1NTI3MGJhMC02Yjg2LTRlMTItYTc4ZS05YzY5" \
    #              "ZDE3ZTEwZWKqVGFrZVByb2ZpdMQBgA=="
    #
    #     body = b64decode(base64)
    #
    #     # Act
    #     result = self.serializer.deserialize(body)
    #
    #     # Assert
    #     self.assertTrue(isinstance(result, SubmitBracketOrder))

    def test_serialize_and_deserialize_submit_bracket_order_with_take_profit_commands(self):
        # Arrange
        entry_order = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(1.00000, 5))

        bracket_order = self.order_factory.bracket(
            entry_order,
            stop_loss=Price(0.99900, 5),
            take_profit=Price(1.00010, 5))

        command = SubmitBracketOrder(
            self.trader_id,
            self.account_id,
            StrategyId("SCALPER", "01"),
            ClientPositionId("P-123456"),
            bracket_order,
            uuid4(),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(command, deserialized)
        self.assertEqual(bracket_order, deserialized.bracket_order)
        print(b64encode(serialized))
        print(command)

    def test_serialize_and_deserialize_modify_order_commands(self):
        # Arrange
        command = ModifyOrder(
            self.trader_id,
            self.account_id,
            ClientOrderId("O-123456"),
            Quantity(100000),
            Price(1.00001, 5),
            uuid4(),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(command, deserialized)
        print(b64encode(serialized))
        print(command)

    def test_serialize_and_deserialize_cancel_order_commands(self):
        # Arrange
        command = CancelOrder(
            self.trader_id,
            self.account_id,
            ClientOrderId("O-123456"),
            uuid4(),
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

    def test_serialize_and_deserialize_order_initialized_events(self):
        # Arrange
        options = {'Price': '1.0005'}

        event = OrderInitialized(
            ClientOrderId("O-123456"),
            AUDUSD_FXCM,
            OrderSide.SELL,
            OrderType.STOP,
            Quantity(100000),
            TimeInForce.DAY,
            uuid4(),
            UNIX_EPOCH,
            options=options)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)
        self.assertEqual(options, event.options)

    def test_serialize_and_deserialize_order_submitted_events(self):
        # Arrange
        event = OrderSubmitted(
            self.account_id,
            ClientOrderId("O-123456"),
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_serialize_and_deserialize_order_invalid_events(self):
        # Arrange
        event = OrderInvalid(
            ClientOrderId("O-123456"),
            "OrderId already exists",
            uuid4(),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_serialize_and_deserialize_order_denied_events(self):
        # Arrange
        event = OrderDenied(
            ClientOrderId("O-123456"),
            "Exceeds risk for FX",
            uuid4(),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_serialize_and_deserialize_order_accepted_events(self):
        # Arrange
        event = OrderAccepted(
            self.account_id,
            ClientOrderId("O-123456"),
            OrderId("B-123456"),
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_serialize_and_deserialize_order_rejected_events(self):
        # Arrange
        event = OrderRejected(
            self.account_id,
            ClientOrderId("O-123456"),
            UNIX_EPOCH,
            "ORDER_ID_INVALID",
            uuid4(),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_serialize_and_deserialize_order_working_events(self):
        # Arrange
        event = OrderWorking(
            self.account_id,
            ClientOrderId("O-123456"),
            OrderId("B-123456"),
            AUDUSD_FXCM,
            OrderSide.SELL,
            OrderType.STOP,
            Quantity(100000),
            Price(1.50000, 5),
            TimeInForce.DAY,
            None,
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_serialize_and_deserialize_order_working_events_with_expire_time(self):
        # Arrange
        event = OrderWorking(
            self.account_id,
            ClientOrderId("O-123456"),
            OrderId("B-123456"),
            AUDUSD_FXCM,
            OrderSide.SELL,
            OrderType.STOP,
            Quantity(100000),
            Price(1.50000, 5),
            TimeInForce.DAY,
            UNIX_EPOCH,
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_serialize_and_deserialize_order_cancelled_events(self):
        # Arrange
        event = OrderCancelled(
            self.account_id,
            ClientOrderId("O-123456"),
            OrderId("1"),
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_serialize_and_deserialize_order_cancel_reject_events(self):
        # Arrange
        event = OrderCancelReject(
            self.account_id,
            ClientOrderId("O-123456"),
            UNIX_EPOCH,
            "RESPONSE",
            "ORDER_DOES_NOT_EXIST",
            uuid4(),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_serialize_and_deserialize_order_modified_events(self):
        # Arrange
        event = OrderModified(
            self.account_id,
            ClientOrderId("O-123456"),
            OrderId("1"),
            Quantity(100000),
            Price(0.80010, 5),
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_serialize_and_deserialize_order_expired_events(self):
        # Arrange
        event = OrderExpired(
            self.account_id,
            ClientOrderId("O-123456"),
            OrderId("1"),
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_serialize_and_deserialize_order_partially_filled_events(self):
        # Arrange
        event = OrderPartiallyFilled(
            self.account_id,
            ClientOrderId("O-123456"),
            OrderId("1"),
            ExecutionId("E123456"),
            PositionId("T123456"),
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(50000),
            Quantity(50000),
            Price(1.00000, 5),
            Money(0., Currency.USD),
            LiquiditySide.MAKER,
            Currency.USD,
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_serialize_and_deserialize_order_filled_events(self):
        # Arrange
        event = OrderFilled(
            self.account_id,
            ClientOrderId("O-123456"),
            OrderId("1"),
            ExecutionId("E123456"),
            PositionId("T123456"),
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000),
            Price(1.00000, 5),
            Money(0., Currency.USD),
            LiquiditySide.TAKER,
            Currency.USD,
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    # TODO: Breaking changes to C# side (change of event name to AccountState)
    # def test_deserialize_account_state_events_from_csharp(self):
    #     # Arrange
    #     # Base64 bytes string from C# MsgPack.Cli
    #     base64 = "jKRUeXBlsUFjY291bnRTdGF0ZUV2ZW50oklk2SQyYTk4NjM5ZC1lMzJkLTQ" \
    #              "xMDctYmRmMC1hYTU2ODUwMjFiOGOpVGltZXN0YW1wuDE5NzAtMDEtMDFUMD" \
    #              "A6MDA6MDAuMDAwWqlBY2NvdW50SWS2RlhDTS1EMTIzNDU2LVNJTVVMQVRFR" \
    #              "KhDdXJyZW5jeaNVU0SrQ2FzaEJhbGFuY2WmMTAwMDAwrENhc2hTdGFydERh" \
    #              "eaYxMDAwMDCvQ2FzaEFjdGl2aXR5RGF5oTC1TWFyZ2luVXNlZExpcXVpZGF" \
    #              "0aW9uoTC1TWFyZ2luVXNlZE1haW50ZW5hbmNloTCrTWFyZ2luUmF0aW+hML" \
    #              "BNYXJnaW5DYWxsU3RhdHVzoU4="
    #
    #     body = b64decode(base64)
    #
    #     # Act
    #     result = self.serializer.deserialize(body)
    #
    #     # Assert
    #     self.assertTrue(isinstance(result, AccountState))
    #     self.assertTrue(isinstance(result.id, UUID))
    #     self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.timestamp)

    # def test_deserialize_order_invalid_events_from_csharp(self):
    #     # Arrange
    #     # Base64 bytes string from C# MsgPack.Cli
    #     base64 = "haRUeXBlrE9yZGVySW52YWxpZKJJZNkkNzE0N2UyOTktYjkxNC00ZTE0LTg" \
    #              "yYzItN2I0ZmU5MDMwZThiqVRpbWVzdGFtcLgxOTcwLTAxLTAxVDAwOjAwOj" \
    #              "AwLjAwMFqnT3JkZXJJZKhPLTEyMzQ1Nq1JbnZhbGlkUmVhc29ut09yZGVyS" \
    #              "WQgYWxyZWFkeSBleGlzdHMu"
    #
    #     body = b64decode(base64)
    #
    #     # Act
    #     result = self.serializer.deserialize(body)
    #
    #     # Assert
    #     self.assertTrue(isinstance(result, OrderInvalid))
    #     self.assertEqual(ClientOrderId("O-123456"), result.cl_ord_id)
    #     self.assertEqual("OrderId already exists.", result.reason)
    #     self.assertTrue(isinstance(result.client_id, UUID))
    #     self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.timestamp)

    # def test_deserialize_order_denied_events_from_csharp(self):
    #     # Arrange
    #     # Base64 bytes string from C# MsgPack.Cli
    #     base64 = "haRUeXBlq09yZGVyRGVuaWVkoklk2SQ1ZTgyNzllNC02NGY1LTRhNTAtYjB" \
    #              "iYy1iYzI0NzAyMjlkMTGpVGltZXN0YW1wuDE5NzAtMDEtMDFUMDA6MDA6MD" \
    #              "AuMDAwWqdPcmRlcklkqE8tMTIzNDU2rERlbmllZFJlYXNvbrRFeGNlZWRzI" \
    #              "HJpc2sgZm9yIEZYLg=="
    #
    #     body = b64decode(base64)
    #
    #     # Act
    #     result = self.serializer.deserialize(body)
    #
    #     # Assert
    #     self.assertTrue(isinstance(result, OrderDenied))
    #     self.assertEqual(ClientOrderId("O-123456"), result.cl_ord_id)
    #     self.assertEqual("Exceeds risk for FX.", result.reason)
    #     self.assertTrue(isinstance(result.client_id, UUID))
    #     self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.timestamp)

    # def test_deserialize_order_submitted_events_from_csharp(self):
    #     # Arrange
    #     # Base64 bytes string from C# MsgPack.Cli
    #     base64 = "hqRUeXBlrk9yZGVyU3VibWl0dGVkoklk2SQxMThhZjIyZC1jMGQwLTQwNDE" \
    #              "tOWQzMS0xYjI4ZWJiYmEzMjCpVGltZXN0YW1wuDE5NzAtMDEtMDFUMDA6MD" \
    #              "A6MDAuMDAwWqdPcmRlcklkqE8tMTIzNDU2qUFjY291bnRJZLJGWENNLTAyO" \
    #              "DUxOTA4LURFTU+tU3VibWl0dGVkVGltZbgxOTcwLTAxLTAxVDAwOjAwOjAw" \
    #              "LjAwMFo="
    #
    #     body = b64decode(base64)
    #
    #     # Act
    #     result = self.serializer.deserialize(body)
    #
    #     # Assert
    #     self.assertTrue(isinstance(result, OrderSubmitted))
    #     self.assertEqual(ClientOrderId("O-123456"), result.cl_ord_id)
    #     self.assertEqual(AccountId('FXCM', "02851908", AccountType.DEMO), result.account_id)
    #     self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.submitted_time)
    #     self.assertTrue(isinstance(result.client_id, UUID))
    #     self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.timestamp)

    # def test_deserialize_order_accepted_events_from_csharp(self):
    #     # Arrange
    #     # Base64 bytes string from C# MsgPack.Cli
    #     base64 = "iKRUeXBlrU9yZGVyQWNjZXB0ZWSiSWTZJDIzMWFiNjc2LWM4NzItNGJkNC0" \
    #              "4NmNkLTAwYWMzOGM1Zjc2MqlUaW1lc3RhbXC4MTk3MC0wMS0wMVQwMDowMD" \
    #              "owMC4wMDBaqUFjY291bnRJZLJGWENNLTAyODUxOTA4LURFTU+nT3JkZXJJZ" \
    #              "KhPLTEyMzQ1Nq1PcmRlcklkQnJva2VyqUJPLTEyMzQ1NqVMYWJlbKpURVNU" \
    #              "X09SREVSrEFjY2VwdGVkVGltZbgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFo="
    #
    #     body = b64decode(base64)
    #
    #     # Act
    #     result = self.serializer.deserialize(body)
    #
    #     # Assert
    #     self.assertTrue(isinstance(result, OrderAccepted))
    #     self.assertEqual(AccountId('FXCM', "02851908", AccountType.DEMO), result.account_id)
    #     self.assertEqual(ClientOrderId("O-123456"), result.cl_ord_id)
    #     self.assertEqual(OrderId("BO-123456"), result.cl_ord_id)
    #     self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.accepted_time)
    #     self.assertTrue(isinstance(result.client_id, UUID))
    #     self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.timestamp)

    # def test_deserialize_order_rejected_events_from_csharp(self):
    #     # Arrange
    #     # Base64 bytes string from C# MsgPack.Cli
    #     base64 = "h6RUeXBlrU9yZGVyUmVqZWN0ZWSiSWTZJDFkMWM2NTRmLTQ2MTQtNDFlZC1" \
    #              "iNDBlLWU0YzRlMzc3MmQ2NqlUaW1lc3RhbXC4MTk3MC0wMS0wMVQwMDowMD" \
    #              "owMC4wMDBap09yZGVySWSoTy0xMjM0NTapQWNjb3VudElkskZYQ00tMDI4N" \
    #              "TE5MDgtREVNT6xSZWplY3RlZFRpbWW4MTk3MC0wMS0wMVQwMDowMDowMC4w" \
    #              "MDBarlJlamVjdGVkUmVhc29urUlOVkFMSURfT1JERVI="
    #
    #     body = b64decode(base64)
    #
    #     # Act
    #     result = self.serializer.deserialize(body)
    #
    #     # Assert
    #     self.assertTrue(isinstance(result, OrderRejected))
    #     self.assertEqual(ClientOrderId("O-123456"), result.cl_ord_id)
    #     self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.rejected_time)
    #     self.assertEqual("INVALID_ORDER", result.reason)
    #     self.assertTrue(isinstance(result.client_id, UUID))
    #     self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.timestamp)

    # def test_deserialize_order_working_events_from_csharp(self):
    #     # Arrange
    #     # Base64 bytes string from C# MsgPack.Cli
    #     base64 = "j6RUeXBlrE9yZGVyV29ya2luZ6JJZNkkYzUxNzIzMGItMjM3MC00ZTE2LTg" \
    #              "5YTUtZjA2ZTg1YThmZmU1qVRpbWVzdGFtcLgxOTcwLTAxLTAxVDAwOjAwOj" \
    #              "AwLjAwMFqpQWNjb3VudElkskZYQ00tMDI4NTE5MDgtREVNT6dPcmRlcklkq" \
    #              "E8tMTIzNDU2rU9yZGVySWRCcm9rZXKpQk8tMTIzNDU2plN5bWJvbKtBVURV" \
    #              "U0QuRlhDTaVMYWJlbKFFqU9yZGVyU2lkZaNCdXmpT3JkZXJUeXBlpFN0b3C" \
    #              "oUXVhbnRpdHmmMTAwMDAwpVByaWNloTGrVGltZUluRm9yY2WjREFZqkV4cG" \
    #              "lyZVRpbWWkTm9uZatXb3JraW5nVGltZbgxOTcwLTAxLTAxVDAwOjAwOjAwL" \
    #              "jAwMFo="
    #
    #     body = b64decode(base64)
    #
    #     # Act
    #     result = self.serializer.deserialize(body)
    #
    #     # Assert
    #     self.assertTrue(isinstance(result, OrderWorking))
    #     self.assertEqual(ClientOrderId("O-123456"), result.cl_ord_id)
    #     self.assertEqual(OrderId("BO-123456"), result.cl_ord_id)
    #     self.assertEqual(AccountId('FXCM', "02851908", AccountType.DEMO), result.account_id)
    #     self.assertEqual(Symbol("AUDUSD", Venue('FXCM')), result.symbol)
    #     self.assertEqual(OrderType.STOP, result.order_type)
    #     self.assertEqual(Quantity(100000), result.quantity)
    #     self.assertEqual(Price(1, 1), result.price)
    #     self.assertEqual(TimeInForce.DAY, result.time_in_force)
    #     self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.working_time)
    #     self.assertTrue(isinstance(result.client_id, UUID))
    #     self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.timestamp)
    #     self.assertIsNone(result.expire_time)

    # def test_deserialize_order_working_events_with_expire_time_from_csharp(self):
    #     # Arrange
    #     # Base64 bytes string from C# MsgPack.Cli
    #     base64 = "j6RUeXBlrE9yZGVyV29ya2luZ6JJZNkkZGMyNjVmZjAtMWY1Ny00ZmUzLWJ" \
    #              "iY2UtMDZmN2M3YjQ2MjA4qVRpbWVzdGFtcLgxOTcwLTAxLTAxVDAwOjAwOj" \
    #              "AwLjAwMFqpQWNjb3VudElkskZYQ00tMDI4NTE5MDgtREVNT6dPcmRlcklkq" \
    #              "E8tMTIzNDU2rU9yZGVySWRCcm9rZXKpQk8tMTIzNDU2plN5bWJvbKtBVURV" \
    #              "U0QuRlhDTaVMYWJlbKFFqU9yZGVyU2lkZaNCdXmpT3JkZXJUeXBlpFN0b3C" \
    #              "oUXVhbnRpdHmmMTAwMDAwpVByaWNloTGrVGltZUluRm9yY2WjR1REqkV4cG" \
    #              "lyZVRpbWW4MTk3MC0wMS0wMVQwMDowMTowMC4wMDBaq1dvcmtpbmdUaW1lu" \
    #              "DE5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWg=="
    #
    #     body = b64decode(base64)
    #
    #     # Act
    #     result = self.serializer.deserialize(body)
    #
    #     # Assert
    #     self.assertTrue(isinstance(result, OrderWorking))
    #     self.assertEqual(ClientOrderId("O-123456"), result.cl_ord_id)
    #     self.assertEqual(OrderId("BO-123456"), result.cl_ord_id)
    #     self.assertEqual(AccountId('FXCM', "02851908", AccountType.DEMO), result.account_id)
    #     self.assertEqual(Symbol("AUDUSD", Venue('FXCM')), result.symbol)
    #     self.assertEqual(OrderSide.BUY, result.order_side)
    #     self.assertEqual(OrderType.STOP, result.order_type)
    #     self.assertEqual(Quantity(100000), result.quantity)
    #     self.assertEqual(Price(1, 1), result.price)
    #     self.assertEqual(TimeInForce.GTD, result.time_in_force)
    #     self.assertEqual(datetime(1970, 1, 1, 0, 0, 0, 0, tzinfo=pytz.utc), result.working_time)
    #     self.assertTrue(isinstance(result.client_id, UUID))
    #     self.assertEqual(datetime(1970, 1, 1, 0, 0, 0, 0, tzinfo=pytz.utc), result.timestamp)
    #     self.assertEqual(datetime(1970, 1, 1, 0, 1, 0, 0, pytz.utc), result.expire_time)

    # def test_deserialize_order_cancelled_events_from_csharp(self):
    #     # Arrange
    #     # Base64 bytes string from C# MsgPack.Cli
    #     base64 = "hqRUeXBlrk9yZGVyQ2FuY2VsbGVkoklk2SQ0M2EwY2RiNC03YTUyLTRjYWQ" \
    #              "tYjEyMy04MGZiYmYxNDM3MDmpVGltZXN0YW1wuDE5NzAtMDEtMDFUMDA6MD" \
    #              "A6MDAuMDAwWqdPcmRlcklkqE8tMTIzNDU2qUFjY291bnRJZLJGWENNLTAyO" \
    #              "DUxOTA4LURFTU+tQ2FuY2VsbGVkVGltZbgxOTcwLTAxLTAxVDAwOjAwOjAw" \
    #              "LjAwMFo="
    #
    #     body = b64decode(base64)
    #
    #     # Act
    #     result = self.serializer.deserialize(body)
    #
    #     # Assert
    #     self.assertTrue(isinstance(result, OrderCancelled))
    #     self.assertEqual(ClientOrderId("O-123456"), result.cl_ord_id)
    #     self.assertEqual(AccountId('FXCM', "02851908", AccountType.DEMO), result.account_id)
    #     self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.cancelled_time)
    #     self.assertTrue(isinstance(result.client_id, UUID))
    #     self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.timestamp)

    # def test_deserialize_order_cancel_reject_events_from_csharp(self):
    #     # Arrange
    #     # Base64 bytes string from C# MsgPack.Cli
    #     base64 = "iKRUeXBlxBFPcmRlckNhbmNlbFJlamVjdKJJZMQkOGMxYTdmZWYtOWM1ZS0" \
    #              "0MTk4LWE5NjctYmZjZTkzMmQ4N2VlqVRpbWVzdGFtcMQYMTk3MC0wMS0wMV" \
    #              "QwMDowMDowMC4wMDBaqUFjY291bnRJZMQSRlhDTS0wMjg1MTkwOC1ERU1Pp" \
    #              "09yZGVySWTECE8tMTIzNDU2rFJlamVjdGVkVGltZcQYMTk3MC0wMS0wMVQw" \
    #              "MDowMDowMC4wMDBaslJlamVjdGVkUmVzcG9uc2VUb8QQUkVKRUNUX1JFU1B" \
    #              "PTlNFP65SZWplY3RlZFJlYXNvbsQPT1JERVJfTk9UX0ZPVU5E"
    #
    #     body = b64decode(base64)
    #
    #     # Act
    #     result = self.serializer.deserialize(body)
    #
    #     # Assert
    #     self.assertTrue(isinstance(result, OrderCancelReject))
    #     self.assertEqual(ClientOrderId("O-123456"), result.cl_ord_id)
    #     self.assertEqual(AccountId('FXCM', "02851908", AccountType.DEMO), result.account_id)
    #     self.assertEqual("REJECT_RESPONSE?", result.response_to)
    #     self.assertEqual("ORDER_NOT_FOUND", result.reason)
    #     self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.rejected_time)
    #     self.assertTrue(isinstance(result.client_id, UUID))
    #     self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.timestamp)

    # def test_deserialize_order_modified_events_from_csharp(self):
    #     # Arrange
    #     # Base64 bytes string from C# MsgPack.Cli
    #     base64 = "iaRUeXBlrU9yZGVyTW9kaWZpZWSiSWTZJDFkOGFlMDNkLWExMzYtNDM5ZC0" \
    #              "5ZmRlLTYwNDAzYTU1ZWMzOKlUaW1lc3RhbXC4MTk3MC0wMS0wMVQwMDowMD" \
    #              "owMC4wMDBaqUFjY291bnRJZLJGWENNLTAyODUxOTA4LURFTU+nT3JkZXJJZ" \
    #              "KhPLTEyMzQ1Nq1PcmRlcklkQnJva2VyqUJPLTEyMzQ1NrBNb2RpZmllZFF1" \
    #              "YW50aXR5pjEwMDAwMK1Nb2RpZmllZFByaWNloTKsTW9kaWZpZWRUaW1luDE" \
    #              "5NzAtMDEtMDFUMDA6MDA6MDAuMDAwWg=="
    #
    #     body = b64decode(base64)
    #
    #     # Act
    #     result = self.serializer.deserialize(body)
    #
    #     # Assert
    #     self.assertTrue(isinstance(result, OrderModified))
    #     self.assertEqual(ClientOrderId("O-123456"), result.cl_ord_id)
    #     self.assertEqual(OrderId("BO-123456"), result.cl_ord_id)
    #     self.assertEqual(AccountId('FXCM', "02851908", AccountType.DEMO), result.account_id)
    #     self.assertEqual(Price(2, 1), result.modified_price)
    #     self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.modified_time)
    #     self.assertTrue(isinstance(result.client_id, UUID))
    #     self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.timestamp)

    # def test_deserialize_order_expired_events_from_csharp(self):
    #     # Arrange
    #     # Base64 bytes string from C# MsgPack.Cli
    #     base64 = "hqRUeXBlrE9yZGVyRXhwaXJlZKJJZNkkY2EwOTQ5YTEtNmM0MC00NzVmLWE" \
    #              "wNzQtM2JiYzUzYTI5Y2JkqVRpbWVzdGFtcLgxOTcwLTAxLTAxVDAwOjAwOj" \
    #              "AwLjAwMFqnT3JkZXJJZKhPLTEyMzQ1NqlBY2NvdW50SWSyRlhDTS0wMjg1M" \
    #              "TkwOC1ERU1Pq0V4cGlyZWRUaW1luDE5NzAtMDEtMDFUMDA6MDA6MDAuMDAw" \
    #              "Wg=="
    #
    #     body = b64decode(base64)
    #
    #     # Act
    #     result = self.serializer.deserialize(body)
    #
    #     # Assert
    #     self.assertTrue(isinstance(result, OrderExpired))
    #     self.assertEqual(ClientOrderId("O-123456"), result.cl_ord_id)
    #     self.assertEqual(AccountId('FXCM', "02851908", AccountType.DEMO), result.account_id)
    #     self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.expired_time)
    #     self.assertTrue(isinstance(result.client_id, UUID))
    #     self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.timestamp)

    # TODO: Breaking changes to C# side (LiquiditySide)
    # def test_deserialize_order_partially_filled_events_from_csharp(self):
    #     # Arrange
    #     # Base64 bytes string from C# MsgPack.Cli
    #     base64 = "jqRUeXBltE9yZGVyUGFydGlhbGx5RmlsbGVkoklk2SQwOTk3Nzk1Ny0zMzE" \
    #              "3LTQ3ODgtOGYxOC1lMmEyY2I0ZDljYmSpVGltZXN0YW1wuDE5NzAtMDEtMD" \
    #              "FUMDA6MDA6MDAuMDAwWqlBY2NvdW50SWSyRlhDTS0wMjg1MTkwOC1ERU1Pp" \
    #              "09yZGVySWSoTy0xMjM0NTarRXhlY3V0aW9uSWSnRTEyMzQ1NrBQb3NpdGlv" \
    #              "bklkQnJva2Vyp1AxMjM0NTamU3ltYm9sq0FVRFVTRC5GWENNqU9yZGVyU2l" \
    #              "kZaNCdXmuRmlsbGVkUXVhbnRpdHmlNTAwMDCuTGVhdmVzUXVhbnRpdHmlNT" \
    #              "AwMDCsQXZlcmFnZVByaWNlozIuMKhDdXJyZW5jeaNVU0StRXhlY3V0aW9uV" \
    #              "GltZbgxOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFo="
    #
    #     body = b64decode(base64)
    #
    #     # Act
    #     result = self.serializer.deserialize(body)
    #
    #     # Assert
    #     self.assertTrue(isinstance(result, OrderPartiallyFilled))
    #     self.assertEqual(OrderId("O-123456"), result.order_id)
    #     self.assertEqual(AccountId('FXCM', "02851908", AccountType.DEMO), result.account_id)
    #     self.assertEqual(ExecutionId("E123456"), result.execution_id)
    #     self.assertEqual(PositionIdBroker("P123456"), result.position_id_broker)
    #     self.assertEqual(Symbol("AUDUSD", Venue('FXCM')), result.symbol)
    #     self.assertEqual(840, result.quote_currency)
    #     self.assertEqual(OrderSide.BUY, result.order_side)
    #     self.assertEqual(Quantity(50000), result.filled_quantity)
    #     self.assertEqual(Quantity(50000), result.leaves_quantity)
    #     self.assertEqual(Price(2, 1), result.average_price)
    #     self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.execution_time)
    #     self.assertTrue(isinstance(result.id, UUID))
    #     self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.timestamp)

    # TODO: Breaking changes to C# side (LiquiditySide)
    # def test_deserialize_order_filled_events_from_csharp(self):
    #     # Arrange
    #     # Base64 bytes string from C# MsgPack.Cli
    #     base64 = "jaRUeXBlq09yZGVyRmlsbGVkoklk2SRjYTg4NWZhZi1hNjE3LTQ3ZjUtYTU" \
    #              "yZi0yNjljZGFlZmU4NDepVGltZXN0YW1wuDE5NzAtMDEtMDFUMDA6MDA6MD" \
    #              "AuMDAwWqlBY2NvdW50SWSyRlhDTS0wMjg1MTkwOC1ERU1Pp09yZGVySWSoT" \
    #              "y0xMjM0NTarRXhlY3V0aW9uSWSnRTEyMzQ1NrBQb3NpdGlvbklkQnJva2Vy" \
    #              "p1AxMjM0NTamU3ltYm9sq0FVRFVTRC5GWENNqU9yZGVyU2lkZaNCdXmuRml" \
    #              "sbGVkUXVhbnRpdHmmMTAwMDAwrEF2ZXJhZ2VQcmljZaMyLjCoQ3VycmVuY3" \
    #              "mjVVNErUV4ZWN1dGlvblRpbWW4MTk3MC0wMS0wMVQwMDowMDowMC4wMDBa"
    #
    #     body = b64decode(base64)
    #
    #     # Act
    #     result = self.serializer.deserialize(body)
    #
    #     # Assert
    #     self.assertTrue(isinstance(result, OrderFilled))
    #     self.assertEqual(OrderId("O-123456"), result.order_id)
    #     self.assertEqual(AccountId('FXCM', "02851908", AccountType.DEMO), result.account_id)
    #     self.assertEqual(ExecutionId("E123456"), result.execution_id)
    #     self.assertEqual(PositionIdBroker("P123456"), result.position_id_broker)
    #     self.assertEqual(Symbol("AUDUSD", Venue('FXCM')), result.symbol)
    #     self.assertEqual(840, result.quote_currency)
    #     self.assertEqual(OrderSide.BUY, result.order_side)
    #     self.assertEqual(Quantity(100000), result.filled_quantity)
    #     self.assertEqual(Price(2, 1), result.average_price)
    #     self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.execution_time)
    #     self.assertEqual(datetime(1970, 1, 1, 00, 00, 0, 0, pytz.utc), result.timestamp)


class MsgPackInstrumentSerializerTests(unittest.TestCase):

    def test_serialize_and_deserialize_instrument(self):
        # Arrange
        serializer = BsonInstrumentSerializer()

        instrument = Instrument(
            symbol=Symbol("AUDUSD", Venue('FXCM')),
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
            rollover_interest_buy=Decimal64(0.025, 3),
            rollover_interest_sell=Decimal64(-0.035, 3),
            timestamp=UNIX_EPOCH)

        # Act
        serialized = serializer.serialize(instrument)
        deserialized = serializer.deserialize(serialized)

        # Assert
        self.assertEqual(instrument, deserialized)
        self.assertEqual(instrument.id, deserialized.id)
        self.assertEqual(instrument.symbol, deserialized.symbol)
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
        print("instrument")
        print(b64encode(serialized))

    def test_deserialize_from_csharp(self):
        # Arrange
        # Base64 bytes string from C# MsgPack.Cli
        base64 = "vgEAAAJTeW1ib2wADAAAAEFVRFVTRC5GWENNAAJCcm9rZXJTeW1ib2wACAA" \
                 "AAEFVRC9VU0QAAlF1b3RlQ3VycmVuY3kABAAAAFVTRAACU2VjdXJpdHlUeX" \
                 "BlAAYAAABGb3JleAAQUHJpY2VQcmVjaXNpb24ABQAAABBTaXplUHJlY2lza" \
                 "W9uAAAAAAAQTWluU3RvcERpc3RhbmNlRW50cnkAAAAAABBNaW5TdG9wRGlz" \
                 "dGFuY2UAAAAAABBNaW5MaW1pdERpc3RhbmNlRW50cnkAAAAAABBNaW5MaW1" \
                 "pdERpc3RhbmNlAAAAAAACVGlja1NpemUACAAAADAuMDAwMDEAAlJvdW5kTG" \
                 "90U2l6ZQAFAAAAMTAwMAACTWluVHJhZGVTaXplAAIAAAAxAAJNYXhUcmFkZ" \
                 "VNpemUACQAAADUwMDAwMDAwAAJSb2xsb3ZlckludGVyZXN0QnV5AAIAAAAx" \
                 "AAJSb2xsb3ZlckludGVyZXN0U2VsbAACAAAAMQACVGltZXN0YW1wABkAAAA" \
                 "xOTcwLTAxLTAxVDAwOjAwOjAwLjAwMFoAAkJhc2VDdXJyZW5jeQAEAAAAQV" \
                 "VEAAA="

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

    def test_serialize_and_deserialize_connect_requests(self):
        # Arrange
        client_id = ClientId("Trader-001")
        timestamp = UNIX_EPOCH
        authentication = SessionId.py_create(client_id, timestamp, "None")

        request = Connect(
            ClientId("Trader-001"),
            authentication.value,
            uuid4(),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(request)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertTrue(isinstance(deserialized, Connect))
        self.assertEqual("Trader-001", deserialized.client_id.value)
        self.assertEqual("e5db3dad8222a27e5d2991d11ad65f0f74668a4cfb629e97aa6920a73a012f87", deserialized.authentication)

    def test_serialize_and_deserialize_disconnect_requests(self):
        # Arrange
        request = Disconnect(
            ClientId("Trader-001"),
            SessionId("Trader-001-1970-1-1-0"),
            uuid4(),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(request)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertTrue(isinstance(deserialized, Disconnect))
        self.assertEqual("Trader-001", deserialized.client_id.value)
        self.assertEqual("Trader-001-1970-1-1-0", deserialized.session_id.value)

    def test_serialize_and_deserialize_tick_data_requests(self):
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
            uuid4(),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(request)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertTrue(isinstance(deserialized, DataRequest))
        self.assertEqual("Tick[]", deserialized.query["DataType"])

    def test_serialize_and_deserialize_bar_data_requests(self):
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
            uuid4(),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(request)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertTrue(isinstance(deserialized, DataRequest))
        self.assertEqual("Bar[]", deserialized.query["DataType"])

    def test_serialize_and_deserialize_instrument_requests(self):
        # Arrange
        query = {
            "DataType": "Instrument",
            "Symbol": "AUDUSD.FXCM",
        }

        request = DataRequest(
            query,
            uuid4(),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(request)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertTrue(isinstance(deserialized, DataRequest))
        self.assertEqual("Instrument", deserialized.query["DataType"])

    def test_serialize_and_deserialize_instruments_requests(self):
        # Arrange
        query = {
            "DataType": "Instrument[]",
            "Symbol": 'FXCM',
        }

        request = DataRequest(
            query,
            uuid4(),
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

    def test_serialize_and_deserialize_connected_responses(self):
        # Arrange
        request = Connected(
            "Trader-001 connected to session",
            ServerId("NautilusData.CommandServer"),
            SessionId("3c95b0db407d8b28827d9f2a23cd54048956a35ab1441a54ebd43b2aedf282ea"),
            uuid4(),
            uuid4(),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(request)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertTrue(isinstance(deserialized, Connected))
        self.assertEqual("Trader-001 connected to session", deserialized.message)
        self.assertEqual("NautilusData.CommandServer", deserialized.server_id.value)
        self.assertEqual("3c95b0db407d8b28827d9f2a23cd54048956a35ab1441a54ebd43b2aedf282ea", deserialized.session_id.value)

    def test_serialize_and_deserialize_disconnected_responses(self):
        # Arrange
        request = Disconnected(
            "Trader-001 disconnected from session",
            ServerId("NautilusData.CommandServer"),
            SessionId("3c95b0db407d8b28827d9f2a23cd54048956a35ab1441a54ebd43b2aedf282ea"),
            uuid4(),
            uuid4(),
            UNIX_EPOCH)

        # Act
        serialized = self.serializer.serialize(request)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertTrue(isinstance(deserialized, Disconnected))
        self.assertEqual("Trader-001 disconnected from session", deserialized.message)
        self.assertEqual("NautilusData.CommandServer", deserialized.server_id.value)
        self.assertEqual("3c95b0db407d8b28827d9f2a23cd54048956a35ab1441a54ebd43b2aedf282ea", deserialized.session_id.value)

    def test_serialize_and_deserialize_data_responses(self):
        # Arrange
        data = b'\x01 \x00'
        data_type = "NothingUseful"
        data_encoding = "BSON"

        response = DataResponse(
            data=data,
            data_type="NothingUseful",
            data_encoding=data_encoding,
            correlation_id=uuid4(),
            response_id=uuid4(),
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

    def test_serialize_and_deserialize_log_messages(self):
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
