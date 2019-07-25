# -------------------------------------------------------------------------------------------------
# <copyright file="network.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.common.logger cimport LoggerAdapter


cdef class MQWorker:
    """
    The abstract base class for all MQ workers.
    """
    cdef LoggerAdapter _log
    cdef object _thread
    cdef object _context
    cdef str _service_address
    cdef object _handler
    cdef object _socket
    cdef int _cycles

    cdef readonly str name
    cdef readonly bint is_running

    cpdef void start(self)
    cpdef void send(self, bytes message)
    cpdef void stop(self)
    cpdef void _open_connection(self)
    cpdef void _close_connection(self)


cdef class SubscriberWorker(MQWorker):
    """
    Provides an asynchronous worker thread for ZMQ subscriber messaging.
    """
    cdef str _topic

    cpdef void _consume_messages(self)
