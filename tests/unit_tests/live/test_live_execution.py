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

import time
import unittest

from nautilus_trader.analysis.performance import PerformanceAnalyzer
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.common.portfolio import Portfolio
from nautilus_trader.core.message import MessageType
from nautilus_trader.execution.database import InMemoryExecutionDatabase
from nautilus_trader.live.clock import LiveClock
from nautilus_trader.live.execution_client import LiveExecClient
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.live.factories import LiveUUIDFactory
from nautilus_trader.live.logging import LiveLogger
from nautilus_trader.model.commands import AccountInquiry
from nautilus_trader.model.commands import CancelOrder
from nautilus_trader.model.commands import ModifyOrder
from nautilus_trader.model.commands import SubmitBracketOrder
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.network.compression import BypassCompressor
from nautilus_trader.network.encryption import EncryptionSettings
from nautilus_trader.network.identifiers import ServerId
from nautilus_trader.network.node_servers import MessageServer
from nautilus_trader.serialization.serializers import MsgPackCommandSerializer
from nautilus_trader.serialization.serializers import MsgPackDictionarySerializer
from nautilus_trader.serialization.serializers import MsgPackEventSerializer
from nautilus_trader.serialization.serializers import MsgPackRequestSerializer
from nautilus_trader.serialization.serializers import MsgPackResponseSerializer
from tests.test_kit.strategies import TestStrategy1
from tests.test_kit.stubs import TestStubs

AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()
GBPUSD_FXCM = TestStubs.symbol_gbpusd_fxcm()

UTF8 = "utf8"
LOCALHOST = '127.0.0.1'
TEST_COMMANDS_REQ_PORT = 57555
TEST_COMMANDS_REP_PORT = 57556
TEST_EVENTS_PUB_PORT = 57557


class LiveExecutionTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        trader_id = TraderId("TESTER", "000")
        account_id = TestStubs.account_id()

        clock = LiveClock()
        uuid_factory = LiveUUIDFactory()
        logger = LiveLogger(clock)

        self.command_server = MessageServer(
            server_id=ServerId("CommandServer-001"),
            recv_port=TEST_COMMANDS_REQ_PORT,
            send_port=TEST_COMMANDS_REP_PORT,
            header_serializer=MsgPackDictionarySerializer(),
            request_serializer=MsgPackRequestSerializer(),
            response_serializer=MsgPackResponseSerializer(),
            compressor=BypassCompressor(),
            encryption=EncryptionSettings(),
            clock=clock,
            uuid_factory=uuid_factory,
            logger=LoggerAdapter("CommandServer", logger))

        self.command_serializer = MsgPackCommandSerializer()

        self.command_server_sink = []
        self.command_server.register_handler(MessageType.COMMAND, self.command_handler)
        self.command_server.start()

        time.sleep(0.1)

        self.portfolio = Portfolio(
            clock=clock,
            uuid_factory=uuid_factory,
            logger=logger)

        self.analyzer = PerformanceAnalyzer()

        self.exec_db = InMemoryExecutionDatabase(
            trader_id=trader_id,
            logger=logger)

        self.exec_engine = LiveExecutionEngine(
            trader_id=trader_id,
            account_id=account_id,
            database=self.exec_db,
            portfolio=self.portfolio,
            clock=clock,
            uuid_factory=uuid_factory,
            logger=logger)

        self.exec_engine.handle_event(TestStubs.account_event())

        self.exec_client = LiveExecClient(
            exec_engine=self.exec_engine,
            host=LOCALHOST,
            command_req_port=TEST_COMMANDS_REQ_PORT,
            command_res_port=TEST_COMMANDS_REP_PORT,
            event_pub_port=TEST_EVENTS_PUB_PORT,
            compressor=BypassCompressor(),
            encryption=EncryptionSettings(),
            command_serializer=MsgPackCommandSerializer(),
            header_serializer=MsgPackDictionarySerializer(),
            request_serializer=MsgPackRequestSerializer(),
            response_serializer=MsgPackResponseSerializer(),
            event_serializer=MsgPackEventSerializer(),
            clock=clock,
            uuid_factory=uuid_factory,
            logger=logger)

        self.exec_engine.register_client(self.exec_client)
        self.exec_client.connect()

        time.sleep(0.1)

        self.bar_type = TestStubs.bartype_audusd_1min_bid()
        self.strategy = TestStrategy1(self.bar_type, id_tag_strategy="001")
        self.strategy.register_trader(
            TraderId("TESTER", "000"),
            clock,
            uuid_factory,
            logger)
        self.exec_engine.register_strategy(self.strategy)

    def tearDown(self):
        # Tear Down
        self.exec_client.disconnect()
        self.command_server.stop()
        # Allowing the garbage collector to clean up resources avoids threading
        # errors caused by the continuous disposal of sockets. Thus for testing
        # we're avoiding calling .dispose() on the sockets.

    def command_handler(self, message):
        command = self.command_serializer.deserialize(message)
        self.command_server_sink.append(command)

    def test_send_submit_order_command(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        # Act
        self.strategy.submit_order(order, self.strategy.position_id_generator.generate())

        time.sleep(0.5)
        # # Assert
        self.assertEqual(order, self.strategy.order(order.cl_ord_id))
        self.assertEqual(2, self.command_server.recv_count)
        self.assertEqual(1, self.command_server.sent_count)
        self.assertEqual(SubmitOrder, type(self.command_server_sink[0]))

    def test_send_submit_bracket_order(self):
        # Arrange
        entry_order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        bracket_order = self.strategy.order_factory.bracket(
            entry_order,
            stop_loss=Price(0.99900, 5))

        # Act
        self.strategy.submit_bracket_order(bracket_order, self.strategy.position_id_generator.generate())

        time.sleep(0.5)
        # Assert
        self.assertEqual(bracket_order.entry, self.strategy.order(bracket_order.entry.cl_ord_id))
        self.assertEqual(bracket_order.stop_loss, self.strategy.order(bracket_order.stop_loss.cl_ord_id))
        self.assertEqual(2, self.command_server.recv_count)
        self.assertEqual(1, self.command_server.sent_count)
        self.assertEqual(SubmitBracketOrder, type(self.command_server_sink[0]))

    def test_send_cancel_order_command(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        # Act
        self.strategy.submit_order(order, self.strategy.position_id_generator.generate())
        self.strategy.cancel_order(order)

        time.sleep(1.0)
        # Assert
        self.assertEqual(order, self.strategy.order(order.cl_ord_id))
        self.assertEqual(3, self.command_server.recv_count)
        self.assertEqual(1, self.command_server.sent_count)
        self.assertEqual(SubmitOrder, type(self.command_server_sink[0]))
        self.assertEqual(CancelOrder, type(self.command_server_sink[1]))

    def test_send_modify_order_command(self):
        # Arrange
        order = self.strategy.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(1.00000, 5))

        # Act
        self.strategy.submit_order(order, self.strategy.position_id_generator.generate())
        self.strategy.modify_order(order, Quantity(110000), Price(1.00001, 5))

        time.sleep(1.0)
        # Assert
        self.assertEqual(order, self.strategy.order(order.cl_ord_id))
        self.assertEqual(3, self.command_server.recv_count)
        self.assertEqual(1, self.command_server.sent_count)
        self.assertEqual(SubmitOrder, type(self.command_server_sink[0]))
        self.assertEqual(ModifyOrder, type(self.command_server_sink[1]))

    def test_send_account_inquiry_command(self):
        # Arrange
        # Act
        self.strategy.account_inquiry()

        time.sleep(0.5)
        # Assert
        self.assertEqual(2, self.command_server.recv_count)
        self.assertEqual(1, self.command_server.sent_count)
        self.assertEqual(AccountInquiry, type(self.command_server_sink[0]))
