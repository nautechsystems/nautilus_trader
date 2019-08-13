# -------------------------------------------------------------------------------------------------
# <copyright file="stores.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.common.logger cimport LogMessage
from nautilus_trader.model.events cimport Event, OrderEvent, PositionEvent


cdef class LogStore:
    """
    Provides a log store.
    """
    cdef str _key
    cdef object _process
    cdef object _queue
    cdef object _redis

    cpdef void store(self, LogMessage message)
    cpdef void _process_queue(self)


cdef class EventStore:
    """
    Provides a process and thread safe event store.
    """
    cdef str _key_order_event
    cdef str _key_position_event
    cdef object _process
    cdef object _queue
    cdef object _serializer
    cdef object _redis

    cpdef void store(self, Event event)
    cpdef void _process_queue(self)

    cdef void _store_order_event(self, OrderEvent event)
    cdef void _store_position_event(self, PositionEvent event)
