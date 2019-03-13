#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="messaging.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from inv_trader.common.logger cimport LoggerAdapter


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
