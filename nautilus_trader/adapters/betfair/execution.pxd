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

from nautilus_trader.adapters.betfair.providers cimport BetfairInstrumentProvider
from nautilus_trader.live.execution_client cimport LiveExecutionClient
from nautilus_trader.live.execution_engine cimport LiveExecutionEngine


cdef class BetfairExecutionClient(LiveExecutionClient):
    cdef object _client
    cdef object _stream
    cdef str _account_currency
    cpdef public dict venue_order_id_to_client_order_id
    cpdef public set pending_update_order_client_ids

# -- INTERNAL --------------------------------------------------------------------------------------

    cpdef BetfairInstrumentProvider instrument_provider(self)
    cpdef object client(self)
    cpdef LiveExecutionEngine engine(self)
    cpdef str get_account_currency(self)
    cpdef dict _get_account_details(self)
    cpdef dict _get_account_funds(self)

# -- EVENTS ----------------------------------------------------------------------------------------

    # cdef inline void _on_account_state(self, dict event) except *
    # cdef inline void _on_order_status(self, dict event) except *
    # cdef inline void _on_exec_report(self, dict event) except *
    cpdef void handle_order_stream_update(self, bytes raw) except *
