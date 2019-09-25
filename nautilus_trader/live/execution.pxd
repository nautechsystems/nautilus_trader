# -------------------------------------------------------------------------------------------------
# <copyright file="execution.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.message cimport Command
from nautilus_trader.common.execution cimport ExecutionDatabase, ExecutionEngine, ExecutionClient
from nautilus_trader.serialization.base cimport CommandSerializer, ResponseSerializer, EventSerializer


cdef class RedisExecutionDatabase(ExecutionDatabase):
    cdef readonly str key_trader
    cdef readonly str key_accounts
    cdef readonly str key_orders
    cdef readonly str key_positions
    cdef readonly str key_strategies
    cdef readonly str key_index_order_position      # HASH
    cdef readonly str key_index_order_strategy      # HASH
    cdef readonly str key_index_broker_position     # HASH
    cdef readonly str key_index_position_strategy   # HASH
    cdef readonly str key_index_position_orders     # SET
    cdef readonly str key_index_strategy_orders     # SET
    cdef readonly str key_index_strategy_positions  # SET
    cdef readonly str key_index_orders              # SET
    cdef readonly str key_index_orders_working      # SET
    cdef readonly str key_index_orders_completed    # SET
    cdef readonly str key_index_positions           # SET
    cdef readonly str key_index_positions_open      # SET
    cdef readonly str key_index_positions_closed    # SET

    cdef CommandSerializer _command_serializer
    cdef EventSerializer _event_serializer
    cdef object _redis

    cdef readonly bint OPTION_LOAD_CACHES

    cpdef void load_accounts_cache(self) except *
    cpdef void load_orders_cache(self) except *
    cpdef void load_positions_cache(self) except *
    cdef set _decode_set_to_order_ids(self, set original)
    cdef set _decode_set_to_position_ids(self, set original)
    cdef set _decode_set_to_strategy_ids(self, list original)


cdef class LiveExecutionEngine(ExecutionEngine):
    cdef object _message_bus
    cdef object _thread

    cpdef void _consume_messages(self)


cdef class LiveExecClient(ExecutionClient):
    cdef object _zmq_context

    cdef object _commands_worker
    cdef object _events_worker
    cdef CommandSerializer _command_serializer
    cdef ResponseSerializer _response_serializer
    cdef EventSerializer _event_serializer

    cdef readonly str events_topic

    cpdef void _command_handler(self, Command command) except *
    cpdef void _event_handler(self, str topic, bytes event_bytes) except *
