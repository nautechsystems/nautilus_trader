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

from base64 import b64encode
import unittest

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
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
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.events import OrderSubmitted
from nautilus_trader.model.events import OrderWorking
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ExecutionId
from nautilus_trader.model.identifiers import IdTag
from nautilus_trader.model.identifiers import OrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.order import LimitOrder
from nautilus_trader.model.order import StopMarketOrder
from nautilus_trader.serialization.base import Serializer
from nautilus_trader.serialization.serializers import MsgPackCommandSerializer
from nautilus_trader.serialization.serializers import MsgPackDictionarySerializer
from nautilus_trader.serialization.serializers import MsgPackEventSerializer
from nautilus_trader.serialization.serializers import MsgPackOrderSerializer
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
            Price("1.00000"),
            TimeInForce.DAY,
            expire_time=None,
        )

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
            price=Price("1.00000"),
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH,
            init_id=uuid4(),
            timestamp=UNIX_EPOCH,
        )

        # Act
        serialized = self.serializer.serialize(order)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(order, deserialized)
        print(b64encode(serialized))
        print(order)

    def test_serialize_and_deserialize_stop_orders_with_expire_time(self):
        # Arrange
        order = StopMarketOrder(
            ClientOrderId("O-123456"),
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            price=Price("1.00000"),
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH,
            init_id=uuid4(),
            timestamp=UNIX_EPOCH,
        )

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
        self.venue = Venue("FXCM")
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
            UNIX_EPOCH,
        )

        # Act
        serialized = self.serializer.serialize(command)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, command)
        print(b64encode(serialized))
        print(command)

    def test_serialize_and_deserialize_submit_order_commands(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        command = SubmitOrder(
            self.venue,
            self.trader_id,
            self.account_id,
            StrategyId("SCALPER", "01"),
            PositionId("P-123456"),
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

    def test_serialize_and_deserialize_submit_bracket_order_no_take_profit_commands(self):
        # Arrange
        entry_order = self.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        bracket_order = self.order_factory.bracket(
            entry_order,
            stop_loss=Price("0.99900"),
        )

        command = SubmitBracketOrder(
            self.venue,
            self.trader_id,
            self.account_id,
            StrategyId("SCALPER", "01"),
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

    def test_serialize_and_deserialize_submit_bracket_order_with_take_profit_commands(self):
        # Arrange
        entry_order = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price("1.00000"),
        )

        bracket_order = self.order_factory.bracket(
            entry_order,
            stop_loss=Price("0.99900"),
            take_profit=Price("1.00010"),
        )

        command = SubmitBracketOrder(
            self.venue,
            self.trader_id,
            self.account_id,
            StrategyId("SCALPER", "01"),
            bracket_order,
            uuid4(),
            UNIX_EPOCH,
        )

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
            self.venue,
            self.trader_id,
            self.account_id,
            ClientOrderId("O-123456"),
            Quantity(100000),
            Price("1.00001"),
            uuid4(),
            UNIX_EPOCH,
        )

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
            self.venue,
            self.trader_id,
            self.account_id,
            ClientOrderId("O-123456"),
            uuid4(),
            UNIX_EPOCH,
        )

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
            OrderType.STOP_MARKET,
            Quantity(100000),
            TimeInForce.DAY,
            uuid4(),
            UNIX_EPOCH,
            options=options,
        )

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
            UNIX_EPOCH,
        )

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
            UNIX_EPOCH,
        )

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
            UNIX_EPOCH,
        )

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
            UNIX_EPOCH,
        )

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
            UNIX_EPOCH,
        )

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
            OrderType.STOP_MARKET,
            Quantity(100000),
            Price("1.50000"),
            TimeInForce.DAY,
            None,
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH,
        )

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
            OrderType.STOP_MARKET,
            Quantity(100000),
            Price("1.50000"),
            TimeInForce.DAY,
            UNIX_EPOCH,
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH,
        )

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
            UNIX_EPOCH,
        )

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
            UNIX_EPOCH,
        )

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
            Price("0.80010"),
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH,
        )

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
            UNIX_EPOCH,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)

    def test_serialize_and_deserialize_order_partially_filled_events(self):
        # Arrange
        event = OrderFilled(
            self.account_id,
            ClientOrderId("O-123456"),
            OrderId("1"),
            ExecutionId("E123456"),
            PositionId("T123456"),
            StrategyId("S", "001"),
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(50000),
            Quantity(50000),
            Price("1.00000"),
            Money("0", Currency.USD),
            LiquiditySide.MAKER,
            Currency.AUD,
            Currency.USD,
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH,
        )

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
            StrategyId("S", "001"),
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(100000),
            Quantity(),
            Price("1.00000"),
            Money("0", Currency.USD),
            LiquiditySide.TAKER,
            Currency.AUD,
            Currency.USD,
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH,
        )

        # Act
        serialized = self.serializer.serialize(event)
        deserialized = self.serializer.deserialize(serialized)

        # Assert
        self.assertEqual(deserialized, event)
