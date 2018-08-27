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


from threading import Thread
from inv_trader.messaging import RequestWorker

UTF8 = 'utf8'
LOCAL_HOST = "127.0.0.1"
TEST_PORT = 5557
TEST_ADDRESS = f"tcp://{LOCAL_HOST}:{TEST_PORT}"


def listen(Thread):
    while True:
        print("Listening for message...")
        message = listener.receive_serialized
        print(message)


class RequestWorkerTests(unittest.TestCase):

    def test_can_connect_to_socket(self):
        # Arrange
        context = zmq.Context.instance()
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
        context = zmq.Context.instance()
        response_list = []
        response_handler = response_list.append

        mock_receiver = context.socket(1)
        mock_receiver.bind(TEST_ADDRESS)
        thread = Thread(target=listen, args=(mock_receiver))
        thread.start()
        thread.join()

        # worker = RequestWorker(
        #     "TestRequester",
        #     context,
        #     LOCAL_HOST,
        #     TEST_PORT,
        #     response_handler)

        # # Act
        # worker.run()
        # time.sleep(1)
        # worker.send("hello".encode(UTF8))
        #
        # # Tear Down
        # worker.stop()




