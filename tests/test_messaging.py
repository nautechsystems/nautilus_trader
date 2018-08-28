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

        print("\n")

    def test_can_connect_to_socket(self):
        # Arrange
        context = zmq.Context()
        response_list = []
        response_handler = response_list.append

        worker = RequestWorker(
            "TestRequester",
            context,
            LOCAL_HOST,
            TEST_PORT,
            response_handler)

        # Act
        # Assert (no exception raised)
        worker.run()

        # Tear Down
        worker.stop()

    def test_can_send_one_message_and_receive_response(self):
        # Arrange
        context = zmq.Context()
        response_list = []
        response_handler = response_list.append

        server = MockServer(context, TEST_PORT, response_handler)
        server.start()

        worker = RequestWorker(
            "TestRequester",
            context,
            LOCAL_HOST,
            TEST_PORT,
            response_handler)

        # Act
        worker.start()
        worker.send(b'hello')

        # Tear Down
        worker.stop()
        server.stop()

        self.assertEqual(b'hello', response_list[0])

    def test_can_send_multiple_messages_and_receive_correctly_ordered_responses(self):
        # Arrange
        context = zmq.Context()
        response_list = []
        response_handler = response_list.append

        server = MockServer(context, TEST_PORT, response_handler)
        server.start()

        worker = RequestWorker(
            "TestRequester",
            context,
            LOCAL_HOST,
            TEST_PORT,
            response_handler)

        # Act
        worker.start()
        worker.send(b'hello1')
        worker.send(b'hello2')
        worker.send(b'hello3')

        # Tear Down
        worker.stop()
        server.stop()

        self.assertEqual(b'hello1', response_list[0])
        self.assertEqual(b'hello2', response_list[1])
        self.assertEqual(b'hello3', response_list[2])
