# -------------------------------------------------------------------------------------------------
# <copyright file="node_clients.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.types cimport GUID
from nautilus_trader.core.message cimport MessageType, Message, Request
from nautilus_trader.common.clock cimport Clock, TimeEvent
from nautilus_trader.common.guid cimport GuidFactory
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.network.identifiers cimport ClientId, SessionId
from nautilus_trader.network.compression cimport Compressor
from nautilus_trader.network.socket cimport ClientSocket, SubscriberSocket
from nautilus_trader.network.queue cimport MessageQueueInbound, MessageQueueOutbound
from nautilus_trader.serialization.base cimport DictionarySerializer, RequestSerializer, ResponseSerializer


cdef class ClientNode:
    cdef Clock _clock
    cdef GuidFactory _guid_factory
    cdef LoggerAdapter _log
    cdef Compressor _compressor
    cdef object _message_handler

    cdef readonly ClientId client_id
    cdef readonly int sent_count
    cdef readonly int recv_count

    cpdef void register_handler(self, handler) except *
    cpdef bint is_connected(self)
    cpdef void connect(self) except *
    cpdef void disconnect(self) except *
    cpdef void dispose(self) except *


cdef class MessageClient(ClientNode):
    cdef ClientSocket _socket_outbound
    cdef ClientSocket _socket_inbound
    cdef MessageQueueOutbound _queue_outbound
    cdef MessageQueueInbound _queue_inbound
    cdef DictionarySerializer _header_serializer
    cdef RequestSerializer _request_serializer
    cdef ResponseSerializer _response_serializer
    cdef dict _awaiting_reply

    cdef readonly SessionId session_id

    cpdef void send_request(self, Request request) except *
    cpdef void send_string(self, str message) except *
    cpdef void send_message(self, Message message, bytes body) except *
    cdef void _send(self, MessageType message_type, str class_name, bytes body) except *
    cpdef void _check_connection(self, TimeEvent event) except *
    cpdef void _recv_frames(self, list frames) except *
    cdef void _register_message(self, Message message, int retry=*) except *
    cdef void _deregister_message(self, GUID correlation_id, int retry=*) except *


cdef class MessageSubscriber(ClientNode):
    cdef SubscriberSocket _socket
    cdef MessageQueueInbound _queue

    cpdef void subscribe(self, str topic) except *
    cpdef void unsubscribe(self, str topic) except *
    cpdef void _recv_frames(self, list frames) except *
    cpdef void _no_subscriber_handler(self, str topic, bytes body) except *
