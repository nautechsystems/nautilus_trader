# -------------------------------------------------------------------------------------------------
# <copyright file="test_network_clients.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import time
import zmq

from nautilus_trader.core.message import MessageType
from nautilus_trader.common.logging import LogLevel
from nautilus_trader.network.node_clients import MessageClient, MessageSubscriber
from nautilus_trader.network.node_servers import MessageServer, MessagePublisher
from nautilus_trader.network.compression import CompressorBypass
from nautilus_trader.network.encryption import EncryptionSettings
from nautilus_trader.network.identifiers import ClientId, ServerId, SessionId
from nautilus_trader.serialization.serializers import MsgPackDictionarySerializer
from nautilus_trader.serialization.serializers import MsgPackRequestSerializer, MsgPackResponseSerializer
from nautilus_trader.serialization.serializers import MsgPackCommandSerializer
from nautilus_trader.live.clock import LiveClock
from nautilus_trader.live.guid import LiveGuidFactory
from nautilus_trader.live.logging import LiveLogger
from test_kit.stubs import UNIX_EPOCH
from test_kit.mocks import ObjectStorer

LOCALHOST = "127.0.0.1"
TEST_PORT = 55557


class NetworkIdentifiersTests(unittest.TestCase):

    def test_can_generate_new_session_id(self):
        # Arrange
        client_id = ClientId('Trader-001')

        # Act
        session_id = SessionId.py_create(client_id, UNIX_EPOCH, 'None')

        # Assert
        self.assertEqual('3b1e1b0a1cb40ae6b2e1e02f51f0e7e0c121c92859550f37a72d7fc74cbd002f', session_id.value)


class MessageClientTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        clock = LiveClock()
        guid_factory = LiveGuidFactory()
        logger = LiveLogger(level_console=LogLevel.VERBOSE)
        self.context = zmq.Context()
        self.client_sink = []
        self.server_sink = []

        self.server = MessageServer(
            ServerId("Server-001"),
            TEST_PORT,
            MsgPackDictionarySerializer(),
            MsgPackRequestSerializer(),
            MsgPackResponseSerializer(),
            CompressorBypass(),
            EncryptionSettings(),
            clock,
            guid_factory,
            logger)

        # Register test handlers
        self.server.register_handler(MessageType.STRING, self.server_sink.append)
        self.server.start()

        self.command_serializer = MsgPackCommandSerializer()
        self.server.register_handler(MessageType.COMMAND, self.command_handler)

        self.client = MessageClient(
            ClientId("Trader-001"),
            LOCALHOST,
            TEST_PORT,
            MsgPackDictionarySerializer(),
            MsgPackRequestSerializer(),
            MsgPackResponseSerializer(),
            CompressorBypass(),
            EncryptionSettings(),
            clock,
            guid_factory,
            logger)

        self.client.register_handler(self.client_sink.append)

    def tearDown(self):
        # Tear Down
        time.sleep(0.1)
        self.client.disconnect()
        time.sleep(0.1)
        self.server.stop()
        time.sleep(0.1)
        self.client.dispose()
        self.server.dispose()
        time.sleep(0.1)

    def command_handler(self, message):
        command = self.command_serializer.deserialize(message)
        self.server_sink.append(command)

    def test_can_connect_to_server_and_receive_response(self):
        # Arrange
        # Act
        self.client.connect()

        time.sleep(0.1)

        # Assert
        self.assertTrue(self.client.is_connected())

    def test_can_send_one_string_message(self):
        # Arrange
        self.client.connect()

        # Act
        self.client.send_string('hello')

        time.sleep(0.1)

        # Assert
        self.assertEqual(2, self.client.sent_count)
        self.assertEqual(2, self.client.recv_count)
        self.assertEqual(2, self.server.sent_count)
        self.assertEqual(2, self.server.recv_count)
        self.assertEqual(1, len(self.client_sink))
        self.assertEqual(1, len(self.server_sink))
        self.assertEqual('hello', self.server_sink[0])
        self.assertEqual('OK', self.client_sink[0])

    def test_can_send_multiple_messages_and_receive_correctly_ordered_responses(self):
        # Arrange
        self.client.connect()

        # Act
        self.client.send_string('hello1')
        self.client.send_string('hello2')
        self.client.send_string('hello3')

        time.sleep(0.1)

        # Assert
        self.assertEqual(4, self.client.sent_count)
        self.assertEqual(4, self.client.recv_count)
        self.assertEqual(4, self.server.sent_count)
        self.assertEqual(4, self.server.recv_count)
        self.assertEqual(3, len(self.client_sink))
        self.assertEqual(3, len(self.server_sink))
        self.assertEqual('hello1', self.server_sink[0])
        self.assertEqual('hello2', self.server_sink[1])
        self.assertEqual('hello3', self.server_sink[2])
        self.assertEqual('OK', self.client_sink[0])
        self.assertEqual('OK', self.client_sink[1])
        self.assertEqual('OK', self.client_sink[2])


class SubscriberWorkerTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        clock = LiveClock()
        guid_factory = LiveGuidFactory()
        logger = LiveLogger()
        self.zmq_context = zmq.Context()
        self.response_handler = ObjectStorer()

        self.subscriber = MessageSubscriber(
            ClientId("Subscriber-001"),
            LOCALHOST,
            TEST_PORT,
            CompressorBypass(),
            EncryptionSettings(),
            clock,
            guid_factory,
            logger)

        self.publisher = MessagePublisher(
            ServerId("Publisher-001"),
            TEST_PORT,
            CompressorBypass(),
            EncryptionSettings(),
            clock,
            guid_factory,
            logger)

        self.publisher.start()

    def tearDown(self):
        # Tear Down
        self.subscriber.disconnect()
        self.subscriber.dispose()
        self.publisher.stop()
        self.publisher.dispose()

    def test_can_subscribe_to_topic_with_no_registered_handler(self):
        # Arrange
        self.subscriber.connect()
        self.subscriber.subscribe('test_topic')

        time.sleep(0.1)
        # Act
        self.publisher.publish('test_topic', b'hello subscribers')

        time.sleep(0.1)
        # Assert
        self.assertEqual(1, self.publisher.sent_count)
        self.assertEqual(1, self.subscriber.recv_count)

    def test_can_subscribe_to_topic_and_receive_one_published_message(self):
        # Arrange
        self.subscriber.register_handler(self.response_handler.store_2)
        self.subscriber.connect()
        self.subscriber.subscribe('test_topic')

        time.sleep(0.1)
        # Act
        self.publisher.publish('test_topic', b'hello subscribers')

        time.sleep(0.1)
        # Assert
        self.assertEqual(1, self.publisher.sent_count)
        self.assertEqual(1, self.subscriber.recv_count)
        self.assertEqual(('test_topic', b'hello subscribers'), self.response_handler.get_store()[0])

    def test_can_subscribe_to_topic_and_receive_multiple_published_messages_in_correct_order(self):
        # Arrange
        self.subscriber.register_handler(self.response_handler.store_2)
        self.subscriber.connect()
        self.subscriber.subscribe('test_topic')

        time.sleep(0.1)
        # Act
        self.publisher.publish('test_topic', b'hello1')
        self.publisher.publish('test_topic', b'hello2')
        self.publisher.publish('test_topic', b'hello3')

        time.sleep(0.1)
        # Assert
        self.assertEqual(3, self.publisher.sent_count)
        self.assertEqual(3, self.subscriber.recv_count)
        self.assertEqual(('test_topic', b'hello1'), self.response_handler.get_store()[0])
        self.assertEqual(('test_topic', b'hello2'), self.response_handler.get_store()[1])
        self.assertEqual(('test_topic', b'hello3'), self.response_handler.get_store()[2])
