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

from cpython.datetime cimport datetime

from nautilus_trader.adapters.ccxt.providers cimport CCXTInstrumentProvider
from nautilus_trader.live.execution cimport LiveExecutionClient
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport OrderId


cdef class CCXTExecutionClient(LiveExecutionClient):
    cdef object _client
    cdef CCXTInstrumentProvider _instrument_provider

    cdef dict _account_last_free
    cdef dict _account_last_used
    cdef dict _account_last_total

    cdef dict _active_orders

    cdef object _update_instruments_task
    cdef object _watch_balances_task
    cdef object _watch_orders_task
    cdef object _watch_exec_reports_task

# -- INTERNAL --------------------------------------------------------------------------------------

    cdef inline void _log_ccxt_error(self, ex, str method_name) except *

# -- EVENTS ----------------------------------------------------------------------------------------

    cdef inline void _on_account_state(self, dict event) except *
    cdef inline void _on_order_status(self, dict event) except *
    cdef inline void _on_exec_report(self, dict event) except *
    cdef inline void _on_binance_order_status(self, dict event) except *
    cdef inline void _on_binance_exec_report(self, dict event) except *
    cdef inline void _on_bitmex_order_status(self, dict event) except *
    cdef inline void _on_bitmex_exec_report(self, dict event) except *
    cdef inline void _generate_order_invalid(self, ClientOrderId cl_ord_id, str reason) except *
    cdef inline void _generate_order_submitted(self, ClientOrderId cl_ord_id, datetime timestamp) except *
    cdef inline void _generate_order_rejected(self, ClientOrderId cl_ord_id, str reason, datetime timestamp) except *
    cdef inline void _generate_order_accepted(self, ClientOrderId cl_ord_id, OrderId order_id, datetime timestamp) except *
    cdef inline void _generate_order_filled(self, ClientOrderId cl_ord_id, OrderId order_id, dict event) except *
    cdef inline void _generate_order_cancelled(self, ClientOrderId cl_ord_id, OrderId order_id, datetime timestamp) except *
    cdef inline void _generate_order_expired(self, ClientOrderId cl_ord_id, OrderId order_id, datetime timestamp) except *
