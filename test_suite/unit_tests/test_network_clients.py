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

from nautilus_trader.network.node_clients import MessageClient, MessageSubscriber
from nautilus_trader.network.node_servers import MessageServer, MessagePublisher
from nautilus_trader.network.compression import CompressorBypass
from nautilus_trader.network.encryption import EncryptionConfig
from nautilus_trader.network.identifiers import ClientId, ServerId, SessionId
from nautilus_trader.serialization.serializers import MsgPackRequestSerializer, MsgPackResponseSerializer
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
        self.assertEqual('Trader-001-3b1e1b0a1cb40ae6b2e1e02f51f0e7e0c121c92859550f37a72d7fc74cbd002f', session_id.value)


class MessageClientTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        clock = LiveClock()
        guid_factory = LiveGuidFactory()
        logger = LiveLogger()
        self.context = zmq.Context()
        self.response_handler = ObjectStorer()

        self.server = MessageServer(
            ServerId("Server-001"),
            TEST_PORT,
            4,
            self.context,
            MsgPackRequestSerializer(),
            MsgPackResponseSerializer(),
            CompressorBypass(),
            EncryptionConfig(),
            clock,
            guid_factory,
            logger)

        self.server.start()

        self.client = MessageClient(
            ClientId("Trader-001"),
            LOCALHOST,
            TEST_PORT,
            3,
            self.context,
            self.response_handler,
            MsgPackRequestSerializer(),
            MsgPackResponseSerializer(),
            CompressorBypass(),
            EncryptionConfig(),
            clock,
            guid_factory,
            logger)

    def tearDown(self):
        # Tear Down
        time.sleep(0.1)
        self.client.disconnect()
        self.server.stop()

    # TODO
    # def test_send_message_to_wrong_address_times_out(self):
    #     # Arrange
    #     worker = RequestWorker(
    #         "TestRequester",
    #         "TestResponder",
    #         LOCALHOST,
    #         TEST_PORT + 1,
    #         self.context,
    #         CompressorBypass(),
    #         EncryptionConfig(),
    #         self.logger)
    #
    #     worker.connect()
    #
    #     # Act
    #     response = self.worker.send(b'hello')
    #
    #     # Assert
    #     self.assertEqual(b'OK', response)

    def test_can_connect_to_server_and_receive_response(self):
        # Arrange
        self.client.connect()

        time.sleep(0.1)
        # Act
        #response = self.client.send(b'hello')

        # Assert
        #self.assertEqual(b'OK', response)

    def test_can_send_one_message_and_receive_response(self):
        # Arrange
        self.client.connect()

        # Act
        response = self.client.send(b'hello')

        # Assert
        self.assertEqual(b'OK', response)

    def test_can_send_multiple_messages_and_receive_correctly_ordered_responses(self):
        # Arrange
        self.client.connect()

        # Act
        response1 = self.client.send(b'hello1')
        response2 = self.client.send(b'hello2')
        response3 = self.client.send(b'hello3')

        # Assert
        self.assertEqual(b'OK', response1)
        self.assertEqual(b'OK', response2)
        self.assertEqual(b'OK', response3)


class SubscriberWorkerTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        clock = LiveClock()
        guid_factory = LiveGuidFactory()
        logger = LiveLogger()
        self.zmq_context = zmq.Context()
        self.response_handler = ObjectStorer()

        self.worker = MessageSubscriber(
            ClientId("Subscriber-001"),
            'TestPublisher',
            LOCALHOST,
            TEST_PORT,
            self.zmq_context,
            self.response_handler.store_2,
            CompressorBypass(),
            EncryptionConfig(),
            clock,
            guid_factory,
            logger)

        self.publisher = MessagePublisher(self.zmq_context, TEST_PORT, logger)

    def tearDown(self):
        # Tear Down
        self.worker.disconnect()
        self.worker.dispose()
        self.publisher.stop()

    def test_can_subscribe_to_topic_and_receive_one_published_message(self):
        # Arrange
        self.worker.connect()
        self.worker.subscribe('test_topic')

        time.sleep(0.1)
        # Act
        self.publisher.publish('test_topic', b'hello subscribers')

        time.sleep(0.1)
        # Assert
        self.assertEqual(('test_topic', b'hello subscribers'), self.response_handler.get_store()[0])

    def test_can_subscribe_to_topic_and_receive_multiple_published_messages_in_correct_order(self):
        # Arrange
        self.worker.connect()
        self.worker.subscribe('test_topic')

        time.sleep(0.1)
        # Act
        self.publisher.publish('test_topic', b'hello1')
        self.publisher.publish('test_topic', b'hello2')
        self.publisher.publish('test_topic', b'hello3')

        time.sleep(0.1)
        # Assert
        self.assertEqual(('test_topic', b'hello1'), self.response_handler.get_store()[0])
        self.assertEqual(('test_topic', b'hello2'), self.response_handler.get_store()[1])
        self.assertEqual(('test_topic', b'hello3'), self.response_handler.get_store()[2])
