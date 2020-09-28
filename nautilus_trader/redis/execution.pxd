# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from nautilus_trader.execution.database cimport ExecutionDatabase
from nautilus_trader.serialization.base cimport CommandSerializer
from nautilus_trader.serialization.base cimport EventSerializer


cdef class RedisExecutionDatabase(ExecutionDatabase):
    cdef readonly str key_trader
    cdef readonly str key_accounts
    cdef readonly str key_orders
    cdef readonly str key_positions
    cdef readonly str key_strategies
    cdef readonly str key_index_order_position      # HASH
    cdef readonly str key_index_order_strategy      # HASH
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

    cpdef void load_accounts_cache(self) except *
    cpdef void load_orders_cache(self) except *
    cpdef void load_positions_cache(self) except *
    cdef set _decode_set_to_order_ids(self, set original)
    cdef set _decode_set_to_position_ids(self, set original)
    cdef set _decode_set_to_strategy_ids(self, list original)
