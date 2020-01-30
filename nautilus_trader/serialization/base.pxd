# -------------------------------------------------------------------------------------------------
# <copyright file="base.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.message cimport Command, Event, Request, Response
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.objects cimport Instrument
from nautilus_trader.common.logger cimport LogMessage


cdef class Serializer:
    cdef object _re_camel_to_snake

    cdef str convert_camel_to_snake(self, str value)
    cdef str convert_snake_to_camel(self, str value)

    cpdef str py_convert_camel_to_snake(self, str value)
    cpdef str py_convert_snake_to_camel(self, str value)


cdef class QuerySerializer(Serializer):
    cpdef bytes serialize(self, dict data)
    cpdef dict deserialize(self, bytes data_bytes)


cdef class DataSerializer(Serializer):
    cpdef bytes serialize(self, dict data)
    cpdef dict deserialize(self, bytes data_bytes)


cdef class InstrumentSerializer(Serializer):
    cpdef bytes serialize(self, Instrument instrument)
    cpdef Instrument deserialize(self, bytes instrument_bytes)


cdef class OrderSerializer(Serializer):
    cpdef bytes serialize(self, Order order)
    cpdef Order deserialize(self, bytes order_bytes)


cdef class CommandSerializer(Serializer):
    cpdef bytes serialize(self, Command command)
    cpdef Command deserialize(self, bytes command_bytes)


cdef class EventSerializer(Serializer):
    cpdef bytes serialize(self, Event event)
    cpdef Event deserialize(self, bytes event_bytes)


cdef class RequestSerializer(Serializer):
    cpdef bytes serialize(self, Request request)
    cpdef Request deserialize(self, bytes request_bytes)


cdef class ResponseSerializer(Serializer):
    cpdef bytes serialize(self, Response request)
    cpdef Response deserialize(self, bytes response_bytes)


cdef class LogSerializer(Serializer):
    cpdef bytes serialize(self, LogMessage message)
    cpdef LogMessage deserialize(self, bytes message_bytes)
