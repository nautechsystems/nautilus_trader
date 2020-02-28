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
    UNDEFINED = 0,  # Invalid value
    STRING = 1,
    COMMAND = 2,
    DOCUMENT = 3,
    EVENT = 4,
    REQUEST = 5,
    RESPONSE = 6


cdef inline str message_type_to_string(int value):
    if value == 1:
        return 'STRING'
    elif value == 2:
        return 'COMMAND'
    elif value == 3:
        return 'DOCUMENT'
    elif value == 4:
        return 'EVENT'
    elif value == 5:
        return 'REQUEST'
    elif value == 6:
        return 'RESPONSE'
    else:
        return 'UNDEFINED'


cdef inline MessageType message_type_from_string(str value):
    if value == 'STRING':
        return MessageType.STRING
    elif value == 'COMMAND':
        return MessageType.COMMAND
    elif value == 'DOCUMENT':
        return MessageType.DOCUMENT
    elif value == 'EVENT':
        return MessageType.EVENT
    elif value == 'REQUEST':
        return MessageType.REQUEST
    elif value == 'RESPONSE':
        return MessageType.RESPONSE
    else:
        return MessageType.UNDEFINED


cdef class Message:
    cdef readonly MessageType message_type
    cdef readonly GUID id
    cdef readonly datetime timestamp

    cpdef bint equals(self, Message other)


cdef class Command(Message):
    pass


cdef class Document(Message):
    pass


cdef class Event(Message):
    pass


cdef class Request(Message):
    pass


cdef class Response(Message):
    cdef readonly GUID correlation_id
