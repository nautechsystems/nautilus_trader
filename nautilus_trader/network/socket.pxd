# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/gpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.types cimport Identifier
from nautilus_trader.common.logging cimport LoggerAdapter


cdef class Socket:
    cdef LoggerAdapter _log
    cdef object _socket

    cdef readonly Identifier socket_id
    cdef readonly str network_address

    cpdef void connect(self) except *
    cpdef void disconnect(self) except *
    cpdef void dispose(self) except *
    cpdef bint is_disposed(self)
    cpdef void send(self, list frames) except *
    cpdef list recv(self)


cdef class ClientSocket(Socket):
    pass


cdef class SubscriberSocket(ClientSocket):
    cpdef void subscribe(self, str topic) except *
    cpdef void unsubscribe(self, str topic) except *


cdef class ServerSocket(Socket):
    pass
