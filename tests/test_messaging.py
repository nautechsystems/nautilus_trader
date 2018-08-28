#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_messaging.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import zmq

from inv_trader.messaging import RequestWorker
from test_kit.mocks import MockServer

UTF8 = 'utf8'
LOCAL_HOST = "127.0.0.1"
TEST_PORT = 5557
TEST_ADDRESS = f"tcp://{LOCAL_HOST}:{TEST_PORT}"


class RequestWorkerTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.context = zmq.Context()
        self.response_list = []
        self.response_handler = self.response_list.append

        self.worker = RequestWorker(
            "TestRequester",
            self.context,
            LOCAL_HOST,
            TEST_PORT,
            self.response_handler)

        print("\n")

    def tearDown(self):
        # Tear Down
        self.worker.stop()

    def test_can_send_one_message_and_receive_response(self):
        # Arrange
        server = MockServer(
            self.context,
            TEST_PORT,
            self.response_handler)
        server.start()

        # Act
        self.worker.start()
        self.worker.send(b'hello')

        # Assert
        self.assertEqual(b'hello', self.response_list[0])

        # Tear Down
        server.stop()

    def test_can_send_multiple_messages_and_receive_correctly_ordered_responses(self):
        # Arrange
        server = MockServer(
            self.context,
            TEST_PORT,
            self.response_handler)
        server.start()

        # Act
        self.worker.start()
        self.worker.send(b'hello1')
        self.worker.send(b'hello2')
        self.worker.send(b'hello3')

        # Assert
        self.assertEqual(b'hello1', self.response_list[0])
        self.assertEqual(b'hello2', self.response_list[1])
        self.assertEqual(b'hello3', self.response_list[2])

        # Tear Down
        server.stop()


