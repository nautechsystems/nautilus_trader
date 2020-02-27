# -------------------------------------------------------------------------------------------------
# <copyright file="node_clients.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.message cimport MessageType, Request
from nautilus_trader.network.identifiers cimport ClientId, SessionId
from nautilus_trader.serialization.base cimport RequestSerializer, ResponseSerializer
from nautilus_trader.network.node_base cimport NetworkNode


cdef class ClientNode(NetworkNode):
    cdef object _thread
    cdef object _frames_handler

    cdef readonly ClientId client_id

    cpdef bint is_connected(self)
    cpdef void connect(self) except *
    cpdef void disconnect(self) except *
    cpdef void _handle_frames(self, list frames) except *
    cpdef void _connect_socket(self) except *
    cpdef void _disconnect_socket(self) except *
    cpdef void _consume_messages(self) except *


cdef class MessageClient(ClientNode):
    cdef RequestSerializer _request_serializer
    cdef ResponseSerializer _response_serializer
    cdef object _response_handler

    cdef readonly SessionId session_id

    cpdef void send_request(self, Request request) except *
    cpdef void send(self, MessageType message_type, bytes message) except *


cdef class MessageSubscriber(ClientNode):
    cdef object _sub_handler

    cpdef void subscribe(self, str topic) except *
    cpdef void unsubscribe(self, str topic) except *
