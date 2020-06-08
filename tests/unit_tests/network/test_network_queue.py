# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import unittest
import zmq

from nautilus_trader.common.logging import LogLevel
from nautilus_trader.network.queue import MessageQueueInbound
from nautilus_trader.live.logging import LiveLogger


class CompressorTests(unittest.TestCase):

    def setUp(self):
        # Fixture setup
        logger = LiveLogger(level_console=LogLevel.VERBOSE)

    # def test_message_queue_inbound_send_recv_stress_test(self):
    #     # Arrange
    #     zmq.context = zmq.Context()
    #     socket = zmq.context.instance().socket(zmq.DEALER)  # noqa
    #
    #
    #     queue = MessageQueueInbound(
    #         3,
    #         socket
    #
    #     )
    #
    #
    #     # Act
    #     compressed = compressor.compress(message)
    #     decompressed = compressor.decompress(compressed)
    #
    #     # Assert
    #     self.assertEqual(message, decompressed)
