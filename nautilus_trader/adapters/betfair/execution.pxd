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
from decimal import Decimal

from nautilus_trader.live.execution_client cimport LiveExecutionClient

from nautilus_trader.adapters.betfair.providers cimport BetfairInstrumentProvider
from nautilus_trader.model.identifiers import ClientOrderId, OrderId, ExecutionId
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderId
from nautilus_trader.model.instrument cimport BettingInstrument
from nautilus_trader.model.identifiers import InstrumentId


cdef class BetfairExecutionClient(LiveExecutionClient):
    cdef object _client

    # -- INTERNAL --------------------------------------------------------------------------------------

    # cdef inline void _log_betfair_error(self, ex, str method_name) except *

    # -- EVENTS ----------------------------------------------------------------------------------------

    # cdef inline void _on_account_state(self, dict event) except *
    # cdef inline void _on_order_status(self, dict event) except *
    # cdef inline void _on_exec_report(self, dict event) except *
    cpdef BetfairInstrumentProvider instrument_provider(self)
    cpdef BettingInstrument get_betting_instrument(self, str market_id, str selection_id, str handicap)

    # TODO - for testing purposes
    cpdef generate_order_invalid(self, ClientOrderId cl_ord_id, str reason)
    cpdef generate_order_submitted(self, ClientOrderId cl_ord_id, datetime timestamp)
    cpdef generate_order_rejected(self, ClientOrderId cl_ord_id, str reason, datetime timestamp)
    cpdef generate_order_accepted(self, ClientOrderId cl_ord_id, OrderId order_id, datetime timestamp)
    cpdef generate_order_filled(
        self,
        ClientOrderId cl_ord_id,
        OrderId order_id,
        ExecutionId execution_id,
        InstrumentId instrument_id,
        OrderSide order_side,
        fill_qty: Decimal,
        cum_qty: Decimal,
        leaves_qty: Decimal,
        avg_px: Decimal,
        commission_amount: Decimal,
        str commission_currency,
        LiquiditySide liquidity_side,
        datetime timestamp
    )
