# -------------------------------------------------------------------------------------------------
# <copyright file="workers.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.common.logger cimport LoggerAdapter


cdef class MQWorker:
    cdef LoggerAdapter _log
    cdef str _service_name
    cdef str _service_address
    cdef object _zmq_context
    cdef object _zmq_socket
    cdef int _cycles

    cdef readonly str name

    cpdef void connect(self)
    cpdef void disconnect(self)
    cpdef void dispose(self)


cdef class RequestWorker(MQWorker):
    cpdef bytes send(self, bytes message)


cdef class SubscriberWorker(MQWorker):
    cdef object _thread
    cdef object _handler

    cpdef void subscribe(self, str topic)
    cpdef void unsubscribe(self, str topic)
    cpdef void _consume_messages(self)
