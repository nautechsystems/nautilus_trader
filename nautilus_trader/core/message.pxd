# -------------------------------------------------------------------------------------------------
# <copyright file="message.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.core.types cimport GUID


cpdef enum MessageType:
    UNKNOWN = -1,
    MESSAGE = 0
    COMMAND = 1,
    EVENT = 2,
    REQUEST = 3,
    RESPONSE = 4


cdef class Message:
    """
    The base class for all messages.
    """
    cdef readonly MessageType message_type
    cdef readonly GUID id
    cdef readonly datetime timestamp

    cdef bint equals(self, Message other)


cdef class Command(Message):
    """
    The base class for all commands.
    """


cdef class Event(Message):
    """
    The base class for all events.
    """


cdef class Request(Message):
    """
    The base class for all requests.
    """


cdef class Response(Message):
    """
    The base class for all responses.
    """
    cdef readonly GUID correlation_id
