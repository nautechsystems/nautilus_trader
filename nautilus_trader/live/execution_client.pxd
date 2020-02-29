# -------------------------------------------------------------------------------------------------
# <copyright file="execution_client.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.message cimport Command, Response
from nautilus_trader.common.execution cimport ExecutionClient
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.network.identifiers cimport ClientId
from nautilus_trader.serialization.base cimport CommandSerializer, ResponseSerializer, EventSerializer

cdef class LiveExecClient(ExecutionClient):
    cdef object _zmq_context

    cdef object _commands_worker
    cdef object _events_worker
    cdef CommandSerializer _command_serializer
    cdef ResponseSerializer _response_serializer
    cdef EventSerializer _event_serializer

    cdef readonly TraderId trader_id
    cdef readonly ClientId client_id

    cdef void _send_command(self, Command command) except *
    cdef void _recv_event(self, str topic, bytes event_bytes) except *
