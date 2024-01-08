# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.cache.facade cimport CacheDatabaseFacade
from nautilus_trader.serialization.base cimport Serializer


cdef class CacheDatabaseAdapter(CacheDatabaseFacade):
    cdef str _key_trader
    cdef str _key_general
    cdef str _key_currencies
    cdef str _key_instruments
    cdef str _key_synthetics
    cdef str _key_accounts
    cdef str _key_orders
    cdef str _key_positions
    cdef str _key_actors
    cdef str _key_strategies

    cdef str _key_index_order_ids
    cdef str _key_index_order_position
    cdef str _key_index_order_client
    cdef str _key_index_orders
    cdef str _key_index_orders_open
    cdef str _key_index_orders_closed
    cdef str _key_index_orders_emulated
    cdef str _key_index_orders_inflight
    cdef str _key_index_positions
    cdef str _key_index_positions_open
    cdef str _key_index_positions_closed

    cdef str _key_snapshots_orders
    cdef str _key_snapshots_positions
    cdef str _key_heartbeat

    cdef Serializer _serializer
    cdef object _backing
