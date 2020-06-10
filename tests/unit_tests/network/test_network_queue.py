# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/gpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
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
