# -------------------------------------------------------------------------------------------------
# <copyright file="message.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.core.types cimport GUID


cpdef enum MessageType:
    UNDEFINED = -1,  # Invalid value
    MESSAGE = 0
    COMMAND = 1,
    EVENT = 2,
    REQUEST = 3,
    RESPONSE = 4


cdef class Message:
    cdef readonly MessageType message_type
    cdef readonly GUID id
    cdef readonly datetime timestamp

    cpdef bint equals(self, Message other)


cdef class Command(Message):
    pass


cdef class Event(Message):
    pass


cdef class Request(Message):
    pass


cdef class Response(Message):
    cdef readonly GUID correlation_id
