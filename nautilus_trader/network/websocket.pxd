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


cdef class WebSocketClient:
    cdef readonly object _loop
    cdef readonly LoggerAdapter _log
    cdef str _ws_url
    cdef dict _ws_kwargs
    cdef object _ws
    cdef object _session
    cdef object _handler
    cdef list _tasks
    cdef bytes _pong_msg
    cdef bint _log_send
    cdef bint _log_recv

    cdef readonly bint is_stopping
    """If the client is stopping.\n\n:returns: `bool`"""
    cdef readonly bint is_running
    """If the client is running.\n\n:returns: `bool`"""
    cdef readonly bint is_connected
    """If the client is connected.\n\n:returns: `bool`"""
    cdef readonly int max_retry_connection
    """The max connection retries.\n\n:returns: `int`"""
    cdef readonly int connection_retry_count
    """The current connection retry count.\n\n:returns: `int`"""
    cdef readonly int unknown_message_count
    """The current unknown message count.\n\n:returns: `int`"""
