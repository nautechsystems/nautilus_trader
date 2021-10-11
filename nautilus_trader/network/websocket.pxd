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

from nautilus_trader.common.logging cimport LoggerAdapter


cdef class WebSocketClient:
    cdef readonly object _loop
    cdef readonly LoggerAdapter _log
    cdef readonly str _ws_url
    cdef readonly dict _ws_kwargs

    cdef object _handler
    cdef object _session
    cdef object _socket
    cdef list _tasks
    cdef bint _running
    cdef bint _stopped
    cdef bint _trigger_stop
    cdef readonly int _connection_retry_count
    cdef readonly int _unknown_message_count
    cdef int _max_retry_connection

    cdef readonly bint is_connected
    """If the client is connected.\n\n:returns: `bool`"""
