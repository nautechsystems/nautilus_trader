#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="msgpack.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from nautilus_trader.core.message cimport Command, Event, Request, Response
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.objects cimport Instrument
from nautilus_trader.common.serialization cimport (
OrderSerializer,
InstrumentSerializer,
EventSerializer,
CommandSerializer,
RequestSerializer,
ResponseSerializer
)


cdef class MsgPackOrderSerializer(OrderSerializer):
    """
    Provides a command serializer for the MessagePack specification
    """
    cpdef bytes serialize(self, Order order)
    cpdef Order deserialize(self, bytes order_bytes)


cdef class MsgPackInstrumentSerializer(InstrumentSerializer):
    """
    Provides an instrument serializer for the MessagePack specification.
    """
    cpdef bytes serialize(self, Instrument instrument)
    cpdef Instrument deserialize(self, bytes instrument_bytes)


cdef class MsgPackCommandSerializer(CommandSerializer):
    """
    Provides a command serializer for the MessagePack specification.
    """
    cpdef OrderSerializer order_serializer

    cpdef bytes serialize(self, Command command)
    cpdef Command deserialize(self, bytes command_bytes)


cdef class MsgPackEventSerializer(EventSerializer):
    """
    Provides an event serializer for the MessagePack specification
    """
    cpdef bytes serialize(self, Event event)
    cpdef Event deserialize(self, bytes event_bytes)


cdef class MsgPackRequestSerializer(RequestSerializer):
    """
    Provides a request serializer for the MessagePack specification
    """
    cpdef bytes serialize(self, Request request)
    cpdef Request deserialize(self, bytes request_bytes)


cdef class MsgPackResponseSerializer(ResponseSerializer):
    """
    Provides a response serializer for the MessagePack specification
    """
    cpdef bytes serialize(self, Response request)
    cpdef Response deserialize(self, bytes response_bytes)
