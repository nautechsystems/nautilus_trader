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
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.common.portfolio import Portfolio
from nautilus_trader.common.uuid import LiveUUIDFactory
from nautilus_trader.core.message import MessageType
from nautilus_trader.enterprise.execution import LiveExecClient
from nautilus_trader.execution.database import InMemoryExecutionDatabase
from nautilus_trader.execution.engine import LiveExecutionEngine
from nautilus_trader.model.commands import AccountInquiry
from nautilus_trader.model.commands import CancelOrder
from nautilus_trader.model.commands import ModifyOrder
from nautilus_trader.model.commands import SubmitBracketOrder
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.enums import OMSType
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
LOCALHOST = "127.0.0.1"


class LiveExecutionTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.trader_id = TraderId("TESTER", "000")
        self.account_id = TestStubs.account_id()

        self.clock = LiveClock()
        self.uuid_factory = LiveUUIDFactory()
        self.logger = LiveLogger(self.clock)

        self.command_serializer = MsgPackCommandSerializer()
        self.command_server_sink = []
        self.command_server = None

        self.portfolio = Portfolio(
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )

        self.analyzer = PerformanceAnalyzer()

        self.exec_db = InMemoryExecutionDatabase(
            trader_id=self.trader_id,
            logger=self.logger,
        )

        self.bar_type = TestStubs.bartype_audusd_1min_bid()
        self.strategy = TestStrategy1(self.bar_type, id_tag_strategy="001")
        self.strategy.register_trader(
            TraderId("TESTER", "000"),
            self.clock,
            self.uuid_factory,
            self.logger,
        )

    def command_handler(self, message):
        command = self.command_serializer.deserialize(message)
        self.command_server_sink.append(command)

    def setup_command_server(
            self,
            commands_req_port: int,
            commands_rep_port: int,
    ):
        return MessageServer(
            server_id=ServerId("CommandServer-001"),
            recv_port=commands_req_port,
            send_port=commands_rep_port,
            header_serializer=MsgPackDictionarySerializer(),
            request_serializer=MsgPackRequestSerializer(),
            response_serializer=MsgPackResponseSerializer(),
            compressor=BypassCompressor(),
            encryption=EncryptionSettings(),
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=LoggerAdapter("CommandServer", self.logger),
        )

    def setup_exec_engine(self):
        return LiveExecutionEngine(
            trader_id=self.trader_id,
            account_id=self.account_id,
            database=self.exec_db,
            oms_type=OMSType.HEDGING,
            portfolio=self.portfolio,
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )

    def setup_exec_client(
            self,
            exec_engine: LiveExecutionEngine,
            commands_req_port: int,
            commands_rep_port: int,
            events_pub_port: int,
    ):
        return LiveExecClient(
            exec_engine=exec_engine,
            host=LOCALHOST,
            command_req_port=commands_req_port,
            command_res_port=commands_rep_port,
            event_pub_port=events_pub_port,
            compressor=BypassCompressor(),
            encryption=EncryptionSettings(),
            command_serializer=MsgPackCommandSerializer(),
            header_serializer=MsgPackDictionarySerializer(),
            request_serializer=MsgPackRequestSerializer(),
            response_serializer=MsgPackResponseSerializer(),
            event_serializer=MsgPackEventSerializer(),
            clock=self.clock,
            uuid_factory=self.uuid_factory,
            logger=self.logger,
        )

    def setup_test_fixture(
            self,
            commands_req_port: int,
            commands_rep_port: int,
            events_pub_port: int,
    ):
        # Setup command server
        self.command_server = self.setup_command_server(
            commands_req_port,
            commands_rep_port,
        )
        self.command_server.register_handler(MessageType.COMMAND, self.command_handler)
        self.command_server.start()
        time.sleep(0.3)

        # Setup execution engine
        exec_engine = self.setup_exec_engine()
        exec_engine.process(TestStubs.account_event())  # Setup account
        exec_engine.register_strategy(self.strategy)

        self.exec_client = self.setup_exec_client(
            exec_engine,
            commands_req_port,
            commands_rep_port,
            events_pub_port,
        )

        # Connect execution engine and client
        exec_engine.register_client(self.exec_client)
        self.exec_client.connect()
        time.sleep(0.3)

    def tearDown(self):
        # Tear Down
        self.exec_client.disconnect()
        self.command_server.stop()

    def test_send_submit_order_command(self):
        # Arrange
        self.setup_test_fixture(
            commands_rep_port=57555,
            commands_req_port=57556,
            events_pub_port=57557,
        )

        order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
        )

        # Act
        self.strategy.submit_order(order)
        time.sleep(0.3)  # Allow order to reach server and server to respond

        # Assert
        self.assertEqual(order, self.exec_db.order(order.cl_ord_id))
        self.assertEqual(2, self.command_server.recv_count)
        self.assertEqual(1, self.command_server.sent_count)
        self.assertEqual(SubmitOrder, type(self.command_server_sink[0]))

    def test_send_submit_bracket_order(self):
        # Arrange
        self.setup_test_fixture(
            commands_rep_port=57565,
            commands_req_port=57566,
            events_pub_port=57567,
        )

        entry_order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
        )

        bracket_order = self.strategy.order_factory.bracket(
            entry_order,
            stop_loss=Price(0.99900, 5),
        )

        # Act
        self.strategy.submit_bracket_order(bracket_order)
        time.sleep(0.3)  # Allow command to reach server and server to respond

        # Assert
        self.assertEqual(bracket_order.entry, self.exec_db.order(bracket_order.entry.cl_ord_id))
        self.assertEqual(bracket_order.stop_loss, self.exec_db.order(bracket_order.stop_loss.cl_ord_id))
        self.assertEqual(2, self.command_server.recv_count)
        self.assertEqual(1, self.command_server.sent_count)
        self.assertEqual(SubmitBracketOrder, type(self.command_server_sink[0]))

    def test_send_cancel_order_command(self):
        # Arrange
        self.setup_test_fixture(
            commands_rep_port=57575,
            commands_req_port=57576,
            events_pub_port=57577,
        )

        order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
        )

        # Act
        self.strategy.submit_order(order)
        self.strategy.cancel_order(order)
        time.sleep(0.3)  # Allow command to reach server and server to respond

        # Assert
        self.assertEqual(order, self.exec_db.order(order.cl_ord_id))
        self.assertEqual(3, self.command_server.recv_count)
        self.assertEqual(1, self.command_server.sent_count)
        self.assertEqual(SubmitOrder, type(self.command_server_sink[0]))
        self.assertEqual(CancelOrder, type(self.command_server_sink[1]))

    def test_send_modify_order_command(self):
        # Arrange
        self.setup_test_fixture(
            commands_rep_port=57585,
            commands_req_port=57586,
            events_pub_port=57587,
        )

        order = self.strategy.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(1.00000, 5),
        )

        # Act
        self.strategy.submit_order(order)
        self.strategy.modify_order(order, Quantity(110000), Price(1.00001, 5))
        time.sleep(0.3)  # Allow command to reach server and server to respond

        # Assert
        self.assertEqual(order, self.exec_db.order(order.cl_ord_id))
        self.assertEqual(3, self.command_server.recv_count)
        self.assertEqual(1, self.command_server.sent_count)
        self.assertEqual(SubmitOrder, type(self.command_server_sink[0]))
        self.assertEqual(ModifyOrder, type(self.command_server_sink[1]))

    def test_send_account_inquiry_command(self):
        # Arrange
        self.setup_test_fixture(
            commands_rep_port=57595,
            commands_req_port=57596,
            events_pub_port=57598,
        )

        # Act
        self.strategy.account_inquiry()
        time.sleep(0.3)  # Allow command to reach server and server to respond

        # Assert
        self.assertEqual(2, self.command_server.recv_count)
        self.assertEqual(1, self.command_server.sent_count)
        self.assertEqual(AccountInquiry, type(self.command_server_sink[0]))
