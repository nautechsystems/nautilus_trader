#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="serialization.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from cpython.datetime cimport datetime

from inv_trader.core.message cimport Command, Event, Request, Response
from inv_trader.model.objects cimport Symbol, Price, Instrument
from inv_trader.model.identifiers cimport Label
from inv_trader.model.order cimport Order


cpdef Symbol parse_symbol(str symbol_string)
cpdef str convert_price_to_string(Price price)
cpdef Price convert_string_to_price(str price_string)
cpdef str convert_label_to_string(Label label)
cpdef Label convert_string_to_label(str label)
cpdef str convert_datetime_to_string(datetime time)
cpdef datetime convert_string_to_datetime(str time_string)


cdef class OrderSerializer:
    """
    The abstract base class for all order serializers.
    """

    cpdef bytes serialize(self, Order order)
    cpdef Order deserialize(self, bytes order_bytes)


cdef class InstrumentSerializer:
    """
    The abstract base class for all instrument serializers.
    """
    cpdef bytes serialize(self, Instrument instrument)
    cpdef Instrument deserialize(self, bytes instrument_bytes)


cdef class CommandSerializer:
    """
    The abstract base class for all command serializers.
    """
    cpdef bytes serialize(self, Command command)
    cpdef Command deserialize(self, bytes command_bytes)


cdef class EventSerializer:
    """
    The abstract base class for all event serializers.
    """
    cpdef bytes serialize(self, Event event)
    cpdef Event deserialize(self, bytes event_bytes)


cdef class RequestSerializer:
    """
    The abstract base class for all request serializers.
    """
    cpdef bytes serialize(self, Request request)
    cpdef Request deserialize(self, bytes request_bytes)


cdef class ResponseSerializer:
    """
    The abstract base class for all response serializers.
    """
    cpdef bytes serialize(self, Response request)
    cpdef Response deserialize(self, bytes response_bytes)
