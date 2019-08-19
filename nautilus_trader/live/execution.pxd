# -------------------------------------------------------------------------------------------------
# <copyright file="execution.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.message cimport Command
from nautilus_trader.common.execution cimport ExecutionDatabase, ExecutionEngine, ExecutionClient
from nautilus_trader.model.events cimport Event, OrderEvent, PositionEvent
from nautilus_trader.serialization.base cimport CommandSerializer, ResponseSerializer, EventSerializer


cdef class RedisExecutionDatabase(ExecutionDatabase):
    """
    Provides a Redis event store for an execution engine.
    """
    cdef str _key_order_event
    cdef str _key_position_event
    cdef object _serializer
    cdef object _process
    cdef object _queue
    cdef object _redis

    cpdef void store(self, Event event)
    cpdef void _process_queue(self)
    cdef void _store_order_event(self, OrderEvent event)
    cdef void _store_position_event(self, PositionEvent event)


cdef class LiveExecutionEngine(ExecutionEngine):
    """
    Provides a process and thread safe live execution engine.
    """
    cdef object _thread
    cdef object _queue

    cpdef void _process_queue(self)


cdef class LiveExecClient(ExecutionClient):
    """
    Provides an execution client for live trading utilizing a ZMQ transport
    to the execution service.
    """
    cdef object _zmq_context

    cdef object _commands_worker
    cdef object _events_worker
    cdef CommandSerializer _command_serializer
    cdef ResponseSerializer _response_serializer
    cdef EventSerializer _event_serializer

    cdef readonly str events_topic

    cdef void _command_handler(self, Command command)
    cdef void _event_handler(self, str topic, bytes event_bytes)
