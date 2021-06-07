# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from libc.stdint cimport int64_t

from nautilus_trader.core.uuid cimport UUID


cpdef enum MessageType:
    STRING = 1,
    COMMAND = 2,
    DOCUMENT = 3,
    EVENT = 4,
    REQUEST = 5,
    RESPONSE = 6,


cpdef str message_type_to_str(int value)
cpdef MessageType message_type_from_str(str value)


cdef class Message:
    cdef readonly MessageType type
    """The generic message type.\n\n:returns: `MessageType`"""
    cdef readonly UUID id
    """The message identifier.\n\n:returns: `UUID`"""
    cdef readonly int64_t timestamp_ns
    """The UNIX timestamp (nanos) of message initialization.\n\n:returns: `int64`"""


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
    """The response correlation identifier.\n\n:returns: `UUID`"""
