# -------------------------------------------------------------------------------------------------
# <copyright file="workers.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.message cimport MessageType
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.guid cimport GuidFactory
from nautilus_trader.common.logger cimport LoggerAdapter
from nautilus_trader.network.identifiers cimport ClientId, SessionId
from nautilus_trader.serialization.base cimport RequestSerializer, ResponseSerializer
from nautilus_trader.network.compression cimport Compressor


cdef class MQWorker:
    cdef Clock _clock
    cdef GuidFactory _guid_factory
    cdef LoggerAdapter _log
    cdef str _server_address
    cdef object _zmq_context
    cdef object _zmq_socket
    cdef Compressor _compressor
    cdef object _thread
    cdef object _frames_handler
    cdef int _expected_frames
    cdef int _cycles

    cdef readonly ClientId client_id

    cpdef bint is_connected(self)
    cpdef bint is_disposed(self)
    cpdef void connect(self) except *
    cpdef void disconnect(self) except *
    cpdef void dispose(self) except *
    cpdef void _handle_frames(self, list frames) except *
    cpdef void _connect_socket(self) except *
    cpdef void _disconnect_socket(self) except *
    cpdef void _consume_messages(self) except *


cdef class DealerWorker(MQWorker):
    cdef RequestSerializer _request_serializer
    cdef ResponseSerializer _response_serializer
    cdef object _response_handler

    cdef readonly SessionId session_id

    cpdef void send(self, MessageType message_type, bytes message) except *


cdef class SubscriberWorker(MQWorker):
    cdef object _sub_handler

    cdef readonly str service_name

    cpdef void subscribe(self, str topic) except *
    cpdef void unsubscribe(self, str topic) except *
