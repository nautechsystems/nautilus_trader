# -------------------------------------------------------------------------------------------------
# <copyright file="queue.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import queue
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
        Initializes a new instance of the MessageQueueOutbound class.

        Parameters
        ----------
        socket: Socket
            The socket for the queue.
        logger : LoggerAdapter
            The logger for the component.
        """
        self._log = logger
        self._socket = socket
        self._queue = queue.Queue()
        self._thread = threading.Thread(target=self._get_loop, daemon=True)

        self._thread.start()

    cpdef void send(self, list frames) except *:
        self._queue.put_nowait(frames)

    cpdef void _get_loop(self) except *:
        self._log.debug("Outbound loop starting...")

        while True:
            self._socket.send(self._queue.get())


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
        Initializes a new instance of the MessageQueueInbound class.

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
        self._queue = queue.Queue()
        self._thread_put = threading.Thread(target=self._put_loop, daemon=True)
        self._thread_get = threading.Thread(target=self._get_loop, daemon=True)
        self._frames_receiver = frames_receiver

        self._thread_put.start()
        self._thread_get.start()

    cpdef void _put_loop(self) except *:
        self._log.debug("Inbound receive loop starting...")

        cdef list frames
        while True:
            frames = self._socket.recv()
            if frames is not None:
                self._queue.put_nowait(frames)

    cpdef void _get_loop(self) except *:
        self._log.debug("Inbound handling loop starting...")

        cdef list frames
        cdef int frames_length
        while True:
            frames = self._queue.get()
            frames_length = len(frames)
            if frames_length != self._expected_frames:
                self._log.error(f"Received unexpected frames count {frames_length}, "
                                f"expected {self._expected_frames}")
                return
            self._frames_receiver(frames)
