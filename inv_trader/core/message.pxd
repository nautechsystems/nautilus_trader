#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="message.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from cpython.datetime cimport datetime

from inv_trader.model.identifiers cimport GUID


cdef class Message:
    """
    The base class for all messages.
    """
    cdef readonly GUID id
    cdef readonly datetime timestamp

    cdef bint equals(self, Message other)


cdef class Command(Message):
    """
    The base class for all commands.
    """
    pass


cdef class Event(Message):
    """
    The base class for all events.
    """
    pass


cdef class Request(Message):
    """
    The base class for all requests.
    """
    pass


cdef class Response(Message):
    """
    The base class for all responses.
    """
    cdef readonly GUID correlation_id
