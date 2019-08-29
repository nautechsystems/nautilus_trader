# -------------------------------------------------------------------------------------------------
# <copyright file="base.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.message cimport Command, Event, Request, Response
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.objects cimport Instrument


cdef class QuerySerializer:
    cpdef bytes serialize(self, dict data)
    cpdef dict deserialize(self, bytes data_bytes)


cdef class DataSerializer:
    cpdef bytes serialize(self, dict data)
    cpdef dict deserialize(self, bytes data_bytes)


cdef class InstrumentSerializer:
    cpdef bytes serialize(self, Instrument instrument)
    cpdef Instrument deserialize(self, bytes instrument_bytes)


cdef class OrderSerializer:
    cpdef bytes serialize(self, Order order)
    cpdef Order deserialize(self, bytes order_bytes)


cdef class CommandSerializer:
    cpdef bytes serialize(self, Command command)
    cpdef Command deserialize(self, bytes command_bytes)


cdef class EventSerializer:
    cpdef bytes serialize(self, Event event)
    cpdef Event deserialize(self, bytes event_bytes)


cdef class RequestSerializer:
    cpdef bytes serialize(self, Request request)
    cpdef Request deserialize(self, bytes request_bytes)


cdef class ResponseSerializer:
    cpdef bytes serialize(self, Response request)
    cpdef Response deserialize(self, bytes response_bytes)
