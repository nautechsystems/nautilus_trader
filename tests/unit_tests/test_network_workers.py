#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_messaging.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import time
import zmq

from nautilus_trader.network.workers import RequestWorker, SubscriberWorker
from test_kit.mocks import MockServer, MockPublisher

UTF8 = 'utf8'
LOCAL_HOST = "127.0.0.1"
TEST_PORT = 5557
TEST_ADDRESS = f"tcp://{LOCAL_HOST}:{TEST_PORT}"


class RequestWorkerTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        print("\n")

        self.context = zmq.Context()
        self.response_list = []
        self.response_handler = self.response_list.append

        self.worker = RequestWorker(
            "TestRequester",
            self.context,
            LOCAL_HOST,
            TEST_PORT,
            self.response_handler)

        self.server = MockServer(
            self.context,
            TEST_PORT,
            self.response_handler)

    def tearDown(self):
        # Tear Down
        self.worker.stop()
        self.server.stop()

    def test_can_send_one_message_and_receive_response(self):
        # Arrange
        self.server.start()
        self.worker.start()

        # Act
        self.worker.send(b'hello')

        # Assert
        self.assertEqual(b'hello', self.response_list[0])

    def test_can_send_multiple_messages_and_receive_correctly_ordered_responses(self):
        # Arrange
        self.server.start()
        self.worker.start()

        # Act
        self.worker.send(b'hello1')
        self.worker.send(b'hello2')
        self.worker.send(b'hello3')

        # Assert
        self.assertEqual(b'hello1', self.response_list[0])
        self.assertEqual(b'hello2', self.response_list[1])
        self.assertEqual(b'hello3', self.response_list[2])


class SubscriberWorkerTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        print("\n")

        self.context = zmq.Context()
        self.response_list = []
        self.response_handler = self.response_list.append

        self.worker = SubscriberWorker(
            "TestSubscriber",
            self.context,
            LOCAL_HOST,
            TEST_PORT,
            "test_topic",
            self.response_handler)

        self.publisher = MockPublisher(
            self.context,
            TEST_PORT,
            self.response_handler)

    def tearDown(self):
        # Tear Down
        self.worker.stop()
        self.publisher.stop()

    def test_can_subscribe_to_topic_and_receive_one_published_message(self):
        # Arrange
        self.publisher.start()
        self.worker.start()

        time.sleep(0.3)
        # Act
        self.publisher.publish("test_topic", b'hello subscribers')

        time.sleep(0.3)
        # Assert
        self.assertEqual(b'hello subscribers', self.response_list[0])

    def test_can_subscribe_to_topic_and_receive_multiple_published_messages_in_correct_order(self):
        # Arrange
        self.publisher.start()
        self.worker.start()

        time.sleep(0.3)
        # Act
        self.publisher.publish("test_topic", b'hello1')
        self.publisher.publish("test_topic", b'hello2')
        self.publisher.publish("test_topic", b'hello3')

        time.sleep(0.3)
        # Assert
        self.assertEqual(b'hello1', self.response_list[0])
        self.assertEqual(b'hello2', self.response_list[1])
        self.assertEqual(b'hello3', self.response_list[2])
