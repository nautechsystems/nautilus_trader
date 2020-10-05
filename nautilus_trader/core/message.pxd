# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.core.uuid cimport UUID


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
    cdef readonly UUID id
    cdef readonly datetime timestamp


cdef class Command(Message):
    pass


cdef class Document(Message):
    pass


cdef class Event(Message):
    pass


cdef class Request(Message):
    pass


cdef class Response(Message):
    cdef readonly UUID correlation_id
