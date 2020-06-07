# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
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
