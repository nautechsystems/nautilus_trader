# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from libc.stdint cimport uint64_t

from nautilus_trader.core.uuid cimport UUID4


cdef class Command:
    cdef readonly UUID4 id
    """The command message ID.\n\n:returns: `UUID4`"""
    cdef readonly uint64_t ts_init
    """UNIX timestamp (nanoseconds) when the object was initialized.\n\n:returns: `uint64_t`"""


cdef class Document:
    cdef readonly UUID4 id
    """The document message ID.\n\n:returns: `UUID4`"""
    cdef readonly uint64_t ts_init
    """UNIX timestamp (nanoseconds) when the object was initialized.\n\n:returns: `uint64_t`"""


cdef class Event:
    pass


cdef class Request:
    cdef readonly UUID4 id
    """The request message ID.\n\n:returns: `UUID4`"""
    cdef readonly uint64_t ts_init
    """UNIX timestamp (nanoseconds) when the object was initialized.\n\n:returns: `uint64_t`"""
    cdef readonly object callback
    """The callback for the response.\n\n:returns: `Callable`"""


cdef class Response:
    cdef readonly UUID4 id
    """The response message ID.\n\n:returns: `UUID4`"""
    cdef readonly uint64_t ts_init
    """UNIX timestamp (nanoseconds) when the object was initialized.\n\n:returns: `uint64_t`"""
    cdef readonly UUID4 correlation_id
    """The response correlation ID.\n\n:returns: `UUID4`"""
