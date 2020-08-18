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

import threading

from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.network.socket cimport Socket


cdef class MessageQueueOutbound:
    """
    Provides a non-blocking outbound message queue.
    """

    def __init__(self,
                 Socket socket not None,
                 LoggerAdapter logger not None):
        """
        Initialize a new instance of the MessageQueueOutbound class.

        Parameters
        ----------
        socket: Socket
            The socket for the queue.
        logger : LoggerAdapter
            The logger for the component.
        """
        self._log = logger
        self._socket = socket

    cpdef void send(self, list frames) except *:
        self._socket.send(frames)


cdef class MessageQueueInbound:
    """
    Provides a non-blocking inbound message queue.
    """

    def __init__(self,
                 int expected_frames,
                 Socket socket not None,
                 frames_receiver: callable,
                 LoggerAdapter logger not None):
        """
        Initialize a new instance of the MessageQueueInbound class.

        Parameters
        ----------
        expected_frames : int
            The expected frames received at this queues port.
        socket: Socket
            The socket for the queue.
        frames_receiver : callable
            The handler method for receiving frames.
        logger : LoggerAdapter
            The logger for the component.
        """
        self._log = logger
        self._expected_frames = expected_frames
        self._socket = socket
        self._thread = threading.Thread(target=self._get_loop, daemon=True)
        self._frames_receiver = frames_receiver

        self._thread.start()

    cpdef void _get_loop(self) except *:
        self._log.debug("Inbound handling loop starting...")

        cdef list frames
        cdef int frames_length
        while True:
            frames = self._socket.recv()
            if frames is None:
                self._log.error("Received None from message queue.")
                return

            frames_length = len(frames)
            if frames_length != self._expected_frames:
                self._log.error(f"Received unexpected frames count {frames_length}, "
                                f"expected {self._expected_frames}")
                return

            self._frames_receiver(frames)
