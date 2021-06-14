# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.live.execution_client cimport LiveExecutionClient
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.orders.base cimport Order


cdef class CCXTExecutionClient(LiveExecutionClient):
    cdef object _client

    cdef object _update_instruments_task
    cdef object _watch_balances_task
    cdef object _watch_orders_task
    cdef object _watch_exec_reports_task

    cdef dict _cached_orders
    cdef dict _cached_filled

# -- INTERNAL --------------------------------------------------------------------------------------

    cdef void _log_ccxt_error(self, ex, str method_name) except *

# -- EVENTS ----------------------------------------------------------------------------------------

    cdef void _on_account_state(self, dict event, bint initial=*) except *
    cdef void _on_order_status(self, dict event) except *
    cdef void _on_exec_report(self, dict event) except *
    cdef Money _parse_commission(self, dict event)
    cdef void _cache_order(self, VenueOrderId venue_order_id, Order order) except *
    cdef void _decache_order(self, VenueOrderId venue_order_id) except *


cdef class BinanceCCXTExecutionClient(CCXTExecutionClient):
    pass


cdef class BitmexCCXTExecutionClient(CCXTExecutionClient):
    pass
