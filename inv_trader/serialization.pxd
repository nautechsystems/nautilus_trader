#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="serialization.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False

from cpython.datetime cimport datetime

from inv_trader.commands cimport Command, OrderCommand
from inv_trader.model.identifiers cimport GUID
from inv_trader.model.order cimport Order
from inv_trader.model.events cimport Event, OrderEvent
from inv_trader.common.serialization cimport OrderSerializer, EventSerializer, CommandSerializer


cdef class MsgPackOrderSerializer(OrderSerializer):
    """
    Provides a command serializer for the Message Pack specification
    """
    cpdef bytes serialize(self, Order order)
    cpdef Order deserialize(self, bytes order_bytes)


cdef class MsgPackCommandSerializer(CommandSerializer):
    """
    Provides a command serializer for the Message Pack specification.
    """
    cpdef OrderSerializer order_serializer

    cpdef bytes serialize(self, Command command)
    cpdef Command deserialize(self, bytes command_bytes)
    cdef bytes _serialize_order_command(self, OrderCommand order_command)
    cdef OrderCommand _deserialize_order_command(
            self,
            GUID command_id,
            datetime command_timestamp,
            dict unpacked)


cdef class MsgPackEventSerializer(EventSerializer):
    """
    Provides an event serializer for the Message Pack specification
    """
    cpdef bytes serialize(self, Event event)
    cpdef Event deserialize(self, bytes event_bytes)
    cdef bytes _serialize_order_event(self, OrderEvent order_event)
    cdef OrderEvent _deserialize_order_event(
            self,
            GUID event_id,
            datetime event_timestamp,
            dict unpacked)

