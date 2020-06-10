# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.network.socket cimport Socket


cdef class MessageQueueOutbound:
    cdef LoggerAdapter _log
    cdef Socket _socket
    cdef object _queue
    cdef object _thread

    cpdef void send(self, list frames) except *
    cpdef void _get_loop(self) except *


cdef class MessageQueueInbound:
    cdef LoggerAdapter _log
    cdef int _expected_frames
    cdef Socket _socket
    cdef object _queue
    cdef object _thread_put
    cdef object _thread_get
    cdef object _frames_receiver

    cdef readonly str network_address

    cpdef void _put_loop(self) except *
    cpdef void _get_loop(self) except *
