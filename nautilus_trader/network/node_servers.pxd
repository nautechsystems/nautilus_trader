# -------------------------------------------------------------------------------------------------
# <copyright file="node_servers.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.types cimport GUID
from nautilus_trader.core.message cimport Message, MessageType
from nautilus_trader.network.identifiers cimport ClientId, ServerId
from nautilus_trader.network.node_base cimport NetworkNode
from nautilus_trader.network.queue cimport MessageQueueDuplex, MessageQueueOutbound
from nautilus_trader.network.messages cimport Response
from nautilus_trader.network.messages cimport Connect, Disconnect
from nautilus_trader.serialization.base cimport DictionarySerializer, RequestSerializer, ResponseSerializer


cdef class ServerNode(NetworkNode):

    cdef readonly ServerId server_id

    cpdef void start(self) except *
    cpdef void stop(self) except *
    cpdef void _bind_socket(self) except *
    cpdef void _unbind_socket(self) except *

cdef class MessageServer(ServerNode):
    cdef MessageQueueDuplex _queue
    cdef DictionarySerializer _header_serializer
    cdef RequestSerializer _request_serializer
    cdef ResponseSerializer _response_serializer
    cdef object _thread
    cdef dict _peers
    cdef dict _handlers

    cpdef void register_request_handler(self, handler) except *
    cpdef void register_handler(self, MessageType message_type, handler) except *
    cpdef void send_rejected(self, str rejected_message, GUID correlation_id, ClientId receiver) except *
    cpdef void send_received(self, Message original, ClientId receiver) except *
    cpdef void send_response(self, Response response, ClientId receiver) except *
    cpdef void send_string(self, str message, ClientId receiver) except *
    cdef void _send(self, ClientId receiver, dict header, bytes body) except *
    cpdef void _handle_frames(self, list frames) except *
    cdef void _handle_request(self, bytes payload, ClientId sender) except *
    cdef void _handle_connection(self, Connect request) except *
    cdef void _handle_disconnection(self, Disconnect request) except *


cdef class MessagePublisher(ServerNode):
    cdef MessageQueueOutbound _queue

    cpdef void publish(self, str topic, bytes message) except *
