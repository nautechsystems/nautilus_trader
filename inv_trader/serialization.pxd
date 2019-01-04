#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="serialization.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False

from inv_trader.commands cimport Command
from inv_trader.model.order cimport Order
from inv_trader.model.events cimport Event
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


cdef class MsgPackEventSerializer(EventSerializer):
    """
    Provides an event serializer for the Message Pack specification
    """
    cpdef bytes serialize(self, Event event)
    cpdef Event deserialize(self, bytes event_bytes)
