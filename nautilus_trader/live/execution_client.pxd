# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.message cimport Command
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.common.execution cimport ExecutionClient
from nautilus_trader.network.identifiers cimport ClientId
from nautilus_trader.network.node_clients cimport MessageClient, MessageSubscriber
from nautilus_trader.serialization.base cimport CommandSerializer, EventSerializer

cdef class LiveExecClient(ExecutionClient):
    cdef MessageClient _command_client
    cdef MessageSubscriber _event_subscriber

    cdef CommandSerializer _command_serializer
    cdef EventSerializer _event_serializer

    cdef readonly TraderId trader_id
    cdef readonly ClientId client_id

    cpdef void _send_command(self, Command command) except *
    cpdef void _recv_event(self, str topic, bytes event_bytes) except *
