# -------------------------------------------------------------------------------------------------
# <copyright file="test_network_workers.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import time
import zmq

from nautilus_trader.network.workers import RequestWorker, SubscriberWorker
from test_kit.mocks import MockServer, MockPublisher
from test_kit.objects import ObjectStorer

LOCAL_HOST = "127.0.0.1"
TEST_PORT = 55557
TEST_ADDRESS = f"tcp://{LOCAL_HOST}:{TEST_PORT}"


class RequestWorkerTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        print("\n")

        self.context = zmq.Context()
        self.response_handler = ObjectStorer()

        self.worker = RequestWorker(
            "TestRequester",
            "TestResponder",
            LOCAL_HOST,
            TEST_PORT,
            self.context)

        self.server = MockServer(self.context, TEST_PORT)
        self.server.start()

    def tearDown(self):
        # Tear Down
        self.worker.disconnect()
        self.server.stop()

    def test_can_send_one_message_and_receive_response(self):
        # Arrange
        self.worker.connect()

        # Act
        response = self.worker.send(b'hello')

        # Assert
        self.assertEqual(b'OK', response)

    def test_can_send_multiple_messages_and_receive_correctly_ordered_responses(self):
        # Arrange
        self.worker.connect()

        # Act
        response1 = self.worker.send(b'hello1')
        response2 = self.worker.send(b'hello2')
        response3 = self.worker.send(b'hello3')

        # Assert
        self.assertEqual(b'OK', response1)
        self.assertEqual(b'OK', response2)
        self.assertEqual(b'OK', response3)


class SubscriberWorkerTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        print("\n")

        self.zmq_context = zmq.Context()
        self.response_handler = ObjectStorer()

        self.worker = SubscriberWorker(
            'TestSubscriber',
            'TestPublisher',
            LOCAL_HOST,
            TEST_PORT,
            self.zmq_context,
            self.response_handler.store_2)

        self.publisher = MockPublisher(self.zmq_context, TEST_PORT)

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
