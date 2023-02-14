# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.common.logging cimport LoggerAdapter


cdef class SocketClient:
    cdef readonly object _loop
    cdef readonly LoggerAdapter _log

    cdef object _reader
    cdef object _writer
    cdef object _handler
    cdef bytes _crlf
    cdef str _encoding
    cdef int _incomplete_read_count
    cdef readonly int _reconnection_count # readonly for test

    cdef readonly object host  # TODO(cs): Temporary `object` typing
    """The host for the socket client.\n\n:returns: `str`"""
    cdef readonly object port  # TODO(cs): Temporary `object` typing
    """The port for the socket client.\n\n:returns: `int`"""
    cdef readonly bint ssl
    """If the socket client is using SSL.\n\n:returns: `bool`"""
    cdef readonly bint is_stopping
    """If the client is stopping.\n\n:returns: `bool`"""
    cdef readonly bint is_running
    """If the client is running.\n\n:returns: `bool`"""
    cdef readonly bint is_connected
    """If the client is connected.\n\n:returns: `bool`"""
