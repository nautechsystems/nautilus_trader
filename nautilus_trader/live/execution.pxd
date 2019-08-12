# -------------------------------------------------------------------------------------------------
# <copyright file="execution.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.message cimport Command
from nautilus_trader.common.execution cimport ExecutionClient
from nautilus_trader.serialization.base cimport CommandSerializer, ResponseSerializer, EventSerializer


cdef class LiveExecClient(ExecutionClient):
    """
    Provides an execution client for live trading utilizing a ZMQ transport
    to the execution service.
    """
    cdef object _zmq_context
    cdef object _queue
    cdef object _thread
    cdef object _commands_worker
    cdef object _events_worker
    cdef CommandSerializer _command_serializer
    cdef ResponseSerializer _response_serializer
    cdef EventSerializer _event_serializer

    cdef readonly str events_topic

    cpdef void _process_queue(self)
    cdef void _send_command(self, Command command)
    cdef void _deserialize_event(self, str topic, bytes event_bytes)
