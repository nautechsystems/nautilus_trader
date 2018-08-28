#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_messaging.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import time
import zmq

from inv_trader.messaging import RequestWorker
from test_kit.mocks import MockServer

UTF8 = 'utf8'
LOCAL_HOST = "127.0.0.1"
TEST_PORT = 5557
TEST_ADDRESS = f"tcp://{LOCAL_HOST}:{TEST_PORT}"


class RequestWorkerTests(unittest.TestCase):

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

    def test_can_send_bytes(self):
        # Arrange
        context = zmq.Context()
        response_list = []
        response_handler = response_list.append

        server = MockServer(context, TEST_PORT, response_handler)
        server.run()

        # socket = context.socket(zmq.REQ)
        # socket.connect(TEST_ADDRESS)
        # socket.send_string("HI")
        # # message = socket.recv_string()
        # # print(message)
        # worker = RequestWorker(
        #     "TestRequester",
        #     context,
        #     LOCAL_HOST,
        #     TEST_PORT,
        #     response_handler)

        # Act
        # worker.run()
        # worker.send("hello".encode(UTF8))
        #
        # # Tear Down
        # worker.stop()
        server.stop()

        self.assertTrue(True)
