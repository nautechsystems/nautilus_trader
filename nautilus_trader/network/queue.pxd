# -------------------------------------------------------------------------------------------------
# <copyright file="queue.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.common.logging cimport LoggerAdapter


cdef class MessageQueueDuplex:
    cdef MessageQueueInbound _inbound
    cdef MessageQueueOutbound _outbound

    cdef void send(self, list frames) except *


cdef class MessageQueueInbound:
    cdef LoggerAdapter _log
    cdef int _expected_frames
    cdef object _socket
    cdef object _queue
    cdef object _thread_put
    cdef object _thread_get
    cdef object _frames_handler

    cpdef void _put_loop(self) except *
    cpdef void _get_loop(self) except *


cdef class MessageQueueOutbound:
    cdef LoggerAdapter _log
    cdef object _socket
    cdef object _queue
    cdef object _thread

    cdef void send(self, list frames) except *
    cpdef void _get_loop(self) except *
