# -------------------------------------------------------------------------------------------------
# <copyright file="test_live_execution.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import time

from nautilus_trader.core.message import MessageType
from nautilus_trader.model.enums import OrderSide, Currency
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Quantity, Price
from nautilus_trader.model.commands import SubmitOrder, SubmitAtomicOrder, CancelOrder, ModifyOrder
from nautilus_trader.model.commands import AccountInquiry
from nautilus_trader.common.logging import LogLevel
from nautilus_trader.common.portfolio import Portfolio
from nautilus_trader.common.execution import InMemoryExecutionDatabase
from nautilus_trader.analysis.performance import PerformanceAnalyzer
from nautilus_trader.network.identifiers import ServerId
from nautilus_trader.network.compression import CompressorBypass
from nautilus_trader.network.encryption import EncryptionSettings
from nautilus_trader.network.node_servers import MessageServer
from nautilus_trader.serialization.serializers import MsgPackDictionarySerializer
from nautilus_trader.serialization.serializers import MsgPackRequestSerializer, MsgPackResponseSerializer
from nautilus_trader.serialization.serializers import MsgPackCommandSerializer, MsgPackEventSerializer
from nautilus_trader.live.clock import LiveClock
from nautilus_trader.live.guid import LiveGuidFactory
from nautilus_trader.live.logging import LiveLogger
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.live.execution_client import LiveExecClient
from test_kit.stubs import TestStubs
from test_kit.strategies import TestStrategy1

AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()
GBPUSD_FXCM = TestStubs.symbol_gbpusd_fxcm()

UTF8 = 'utf8'
LOCALHOST = "127.0.0.1"
TEST_COMMANDS_REQ_PORT = 56555
TEST_COMMANDS_REP_PORT = 56556
TEST_EVENTS_PUB_PORT = 56557


class LiveExecutionTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        trader_id = TraderId('TESTER', '000')
        account_id = TestStubs.account_id()

        clock = LiveClock()
        guid_factory = LiveGuidFactory()
        logger = LiveLogger(level_console=LogLevel.VERBOSE)

        self.command_server = MessageServer(
            server_id=ServerId("CommandServer-001"),
            recv_port=TEST_COMMANDS_REQ_PORT,
            send_port=TEST_COMMANDS_REP_PORT,
            header_serializer=MsgPackDictionarySerializer(),
            request_serializer=MsgPackRequestSerializer(),
            response_serializer=MsgPackResponseSerializer(),
            compressor=CompressorBypass(),
            encryption=EncryptionSettings(),
            clock=clock,
            guid_factory=guid_factory,
            logger=logger)

        self.command_serializer = MsgPackCommandSerializer()

        self.command_server_sink = []
        self.command_server.register_handler(MessageType.COMMAND, self.command_handler)
        self.command_server.start()

        time.sleep(0.1)

        self.portfolio = Portfolio(
            currency=Currency.USD,
            clock=clock,
            guid_factory=guid_factory,
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
            guid_factory=guid_factory,
            logger=logger)

        self.exec_engine.handle_event(TestStubs.account_event())

        self.exec_client = LiveExecClient(
            exec_engine=self.exec_engine,
            host=LOCALHOST,
            command_req_port=TEST_COMMANDS_REQ_PORT,
            command_rep_port=TEST_COMMANDS_REP_PORT,
            event_pub_port=TEST_EVENTS_PUB_PORT,
            compressor=CompressorBypass(),
            encryption=EncryptionSettings(),
            command_serializer=MsgPackCommandSerializer(),
            header_serializer=MsgPackDictionarySerializer(),
            request_serializer=MsgPackRequestSerializer(),
            response_serializer=MsgPackResponseSerializer(),
            event_serializer=MsgPackEventSerializer(),
            clock=clock,
            guid_factory=guid_factory,
            logger=logger)

        self.exec_engine.register_client(self.exec_client)
        self.exec_client.connect()

        time.sleep(0.1)

        self.bar_type = TestStubs.bartype_audusd_1min_bid()
        self.strategy = TestStrategy1(self.bar_type, id_tag_strategy='001')
        self.strategy.change_logger(logger)
        self.exec_engine.register_strategy(self.strategy)

    def tearDown(self):
        # Tear Down
        time.sleep(0.1)
        self.exec_client.disconnect()
        time.sleep(0.1)
        self.command_server.stop()
        time.sleep(0.1)
        self.exec_client.dispose()
        self.command_server.dispose()
        time.sleep(0.1)

    def command_handler(self, message):
        command = self.command_serializer.deserialize(message)
        self.command_server_sink.append(command)

    def test_can_send_submit_order_command(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        # Act
        self.strategy.submit_order(order, self.strategy.position_id_generator.generate())

        time.sleep(0.1)
        # # Assert
        self.assertEqual(order, self.strategy.order(order.id))
        self.assertEqual(2, self.command_server.recv_count)
        self.assertEqual(1, self.command_server.sent_count)
        self.assertEqual(SubmitOrder, type(self.command_server_sink[0]))

    def test_can_send_submit_atomic_order(self):
        # Arrange
        atomic_order = self.strategy.order_factory.atomic_market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(0.99900, 5))

        # Act
        self.strategy.submit_atomic_order(atomic_order, self.strategy.position_id_generator.generate())

        time.sleep(0.1)
        # Assert
        self.assertEqual(atomic_order.entry, self.strategy.order(atomic_order.entry.id))
        self.assertEqual(atomic_order.stop_loss, self.strategy.order(atomic_order.stop_loss.id))
        self.assertEqual(2, self.command_server.recv_count)
        self.assertEqual(1, self.command_server.sent_count)
        self.assertEqual(SubmitAtomicOrder, type(self.command_server_sink[0]))

    def test_can_send_cancel_order_command(self):
        # Arrange
        order = self.strategy.order_factory.market(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000))

        # Act
        self.strategy.submit_order(order, self.strategy.position_id_generator.generate())
        self.strategy.cancel_order(order, 'SIGNAL_GONE')

        time.sleep(0.1)
        # Assert
        self.assertEqual(order, self.strategy.order(order.id))
        self.assertEqual(3, self.command_server.recv_count)
        self.assertEqual(1, self.command_server.sent_count)
        self.assertEqual(SubmitOrder, type(self.command_server_sink[0]))
        self.assertEqual(CancelOrder, type(self.command_server_sink[1]))

    def test_can_send_modify_order_command(self):
        # Arrange
        order = self.strategy.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(100000),
            Price(1.00000, 5))

        # Act
        self.strategy.submit_order(order, self.strategy.position_id_generator.generate())
        self.strategy.modify_order(order, Quantity(110000), Price(1.00001, 5))

        time.sleep(0.1)
        # Assert
        self.assertEqual(order, self.strategy.order(order.id))
        self.assertEqual(3, self.command_server.recv_count)
        self.assertEqual(1, self.command_server.sent_count)
        self.assertEqual(SubmitOrder, type(self.command_server_sink[0]))
        self.assertEqual(ModifyOrder, type(self.command_server_sink[1]))

    def test_can_send_account_inquiry_command(self):
        # Arrange
        # Act
        self.strategy.account_inquiry()

        time.sleep(0.1)
        # Assert
        self.assertEqual(2, self.command_server.recv_count)
        self.assertEqual(1, self.command_server.sent_count)
        self.assertEqual(AccountInquiry, type(self.command_server_sink[0]))
