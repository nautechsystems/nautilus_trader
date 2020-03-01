# -------------------------------------------------------------------------------------------------
# <copyright file="queue.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import queue
import threading
import zmq

from nautilus_trader.common.logging cimport LoggerAdapter


cdef class MessageQueueDuplex:
    """
    Provides a non-blocking duplex message queue.
    """

    def __init__(self,
                 int expected_frames,
                 socket not None: zmq.Socket,
                 handler: callable,
                 LoggerAdapter logger not None):
        """
        Initializes a new instance of the MessageQueueDuplex class.

        TODO: Params
        """
        self._inbound = MessageQueueInbound(
            expected_frames,
            socket,
            handler,
            logger)

        self._outbound = MessageQueueOutbound(socket, logger)

    cdef void send(self, list frames) except *:
        self._outbound.send(frames)


cdef class MessageQueueInbound:
    """
    Provides a non-blocking inbound message queue.
    """

    def __init__(self,
                 int expected_frames,
                 socket: zmq.Socket,
                 handler: callable,
                 LoggerAdapter logger not None):
        """
        Initializes a new instance of the DuplexMessageQueue class.

        TODO: Params
        """
        self._log = logger
        self._expected_frames = expected_frames
        self._socket = socket
        self._queue = queue.Queue()
        self._thread_put = threading.Thread(target=self._put_loop, daemon=True)
        self._thread_get = threading.Thread(target=self._get_loop, daemon=True)
        self._handler = handler

        self._thread_put.start()
        self._thread_get.start()

    cpdef void _put_loop(self) except *:
        self._log.debug("Inbound receive loop starting...")

        while True:
            try:
                self._queue.put_nowait(self._socket.recv_multipart())
            except zmq.ZMQError as ex:
                self._log.error(str(ex))
                continue

    cpdef void _get_loop(self) except *:
        self._log.debug("Inbound handling loop starting...")

        cdef list frames
        cdef int frames_length
        while True:
            frames = self._queue.get()
            frames_length = len(frames)
            if frames_length <= 0:
                self._log.error(f'Received zero frames with no reply address.')
                return
            if frames_length != self._expected_frames:
                self._log.error(f"Received unexpected frames count {frames_length}, expected {self._expected_frames}")
                return
            self._handler(frames)


cdef class MessageQueueOutbound:
    """
    Provides a non-blocking inbound message queue.
    """

    def __init__(self,
                 socket: zmq.Socket,
                 LoggerAdapter logger not None):
        """
        Initializes a new instance of the DuplexMessageQueue class.

        TODO: Params
        """
        self._log = logger
        self._socket = socket
        self._queue = queue.Queue()
        self._thread = threading.Thread(target=self._get_loop, daemon=True)

        self._thread.start()

    cdef void send(self, list frames) except *:
        self._queue.put_nowait(frames)

    cpdef void _get_loop(self) except *:
        self._log.debug("Outbound loop starting...")

        while True:
            self._socket.send_multipart(self._queue.get())
