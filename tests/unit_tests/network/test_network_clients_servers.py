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

import unittest
import time
import zmq

from nautilus_trader.core.message import MessageType
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.network.node_clients import MessageClient, MessageSubscriber
from nautilus_trader.network.node_servers import MessageServer, MessagePublisher
from nautilus_trader.network.compression import BypassCompressor
from nautilus_trader.network.encryption import EncryptionSettings
from nautilus_trader.network.identifiers import ClientId, ServerId, SessionId
from nautilus_trader.serialization.serializers import MsgPackDictionarySerializer
from nautilus_trader.serialization.serializers import MsgPackRequestSerializer, MsgPackResponseSerializer
from nautilus_trader.serialization.serializers import MsgPackCommandSerializer
from nautilus_trader.live.clock import LiveClock
from nautilus_trader.live.factories import LiveUUIDFactory
from nautilus_trader.live.logging import LiveLogger
from tests.test_kit.mocks import ObjectStorer
from tests.test_kit.stubs import UNIX_EPOCH

LOCALHOST = "127.0.0.1"
TEST_RECV_PORT = 55657
TEST_SEND_PORT = 55658


class NetworkIdentifiersTests(unittest.TestCase):

    def test_can_generate_new_session_id(self):
        # Arrange
        client_id = ClientId('Trader-001')

        # Act
        session_id = SessionId.py_create(client_id, UNIX_EPOCH, 'None')

        # Assert
        self.assertEqual('e5db3dad8222a27e5d2991d11ad65f0f74668a4cfb629e97aa6920a73a012f87', session_id.value)


class MessageClientTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        clock = LiveClock()
        uuid_factory = LiveUUIDFactory()
        logger = LiveLogger()
        self.context = zmq.Context()
        self.client_sink = []
        self.server_sink = []

        self.server = MessageServer(
            ServerId("Server-001"),
            TEST_RECV_PORT,
            TEST_SEND_PORT,
            MsgPackDictionarySerializer(),
            MsgPackRequestSerializer(),
            MsgPackResponseSerializer(),
            BypassCompressor(),
            EncryptionSettings(),
            clock,
            uuid_factory,
            LoggerAdapter('MessageServer', logger))

        # Register test handlers
        self.server.register_handler(MessageType.STRING, self.server_sink.append)
        self.server.start()

        self.command_serializer = MsgPackCommandSerializer()
        self.server.register_handler(MessageType.COMMAND, self.command_handler)

        self.client = MessageClient(
            ClientId("Trader-001"),
            LOCALHOST,
            TEST_RECV_PORT,
            TEST_SEND_PORT,
            MsgPackDictionarySerializer(),
            MsgPackRequestSerializer(),
            MsgPackResponseSerializer(),
            BypassCompressor(),
            EncryptionSettings(),
            clock,
            uuid_factory,
            LoggerAdapter('MessageClient', logger))

        self.client.register_handler(self.client_sink.append)

    def tearDown(self):
        # Tear Down
        self.client.disconnect()
        self.server.stop()
        # Allowing the garbage collector to clean up resources avoids threading
        # errors caused by the continuous disposal of sockets. Thus for testing
        # we're avoiding calling .dispose() on the sockets.

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


TEST_PUB_PORT = 55559


class SubscriberWorkerTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        clock = LiveClock()
        uuid_factory = LiveUUIDFactory()
        logger = LiveLogger()
        self.zmq_context = zmq.Context()
        self.response_handler = ObjectStorer()

        self.subscriber = MessageSubscriber(
            ClientId("Subscriber-001"),
            LOCALHOST,
            TEST_PUB_PORT,
            BypassCompressor(),
            EncryptionSettings(),
            clock,
            uuid_factory,
            LoggerAdapter('MessageSubscriber', logger))

        self.publisher = MessagePublisher(
            ServerId("Publisher-001"),
            TEST_PUB_PORT,
            BypassCompressor(),
            EncryptionSettings(),
            clock,
            uuid_factory,
            LoggerAdapter('MessagePublisher', logger))

        self.publisher.start()

    def tearDown(self):
        # Tear Down
        self.subscriber.disconnect()
        self.publisher.stop()
        # Allowing the garbage collector to clean up resources avoids threading
        # errors caused by the continuous disposal of sockets. Thus for testing
        # we're avoiding calling .dispose() on the sockets.

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
