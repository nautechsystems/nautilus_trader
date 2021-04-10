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

from decimal import Decimal

from libc.stdint cimport int64_t

from nautilus_trader.common.providers cimport InstrumentProvider
from nautilus_trader.execution.client cimport ExecutionClient
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class LiveExecutionClientFactory:
    pass


cdef class LiveExecutionClient(ExecutionClient):
    cdef object _loop

    cdef InstrumentProvider _instrument_provider
    cdef dict _account_last_free
    cdef dict _account_last_used
    cdef dict _account_last_total

    cdef void _on_reset(self) except *
    cdef inline void _generate_order_invalid(self, ClientOrderId client_order_id, str reason) except *
    cdef inline void _generate_order_submitted(self, ClientOrderId client_order_id, int64_t timestamp_ns) except *
    cdef inline void _generate_order_rejected(self, ClientOrderId client_order_id, str reason, int64_t timestamp_ns) except *
    cdef inline void _generate_order_accepted(self, ClientOrderId client_order_id, VenueOrderId venue_order_id, int64_t timestamp_ns) except *
    cdef inline void _generate_order_filled(
        self,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        ExecutionId execution_id,
        InstrumentId instrument_id,
        OrderSide order_side,
        last_qty: Decimal,
        last_px: Decimal,
        cum_qty: Decimal,
        leaves_qty: Decimal,
        commission_amount: Decimal,
        str commission_currency,
        LiquiditySide liquidity_side,
        int64_t timestamp_ns,
    ) except *
    cdef inline void _generate_order_cancelled(self, ClientOrderId client_order_id, VenueOrderId venue_order_id, int64_t timestamp_ns) except *
    cdef inline void _generate_order_expired(self, ClientOrderId client_order_id, VenueOrderId venue_order_id, int64_t timestamp_ns) except *
    cdef inline void _generate_order_updated(
        self,
        Price price,
        Quantity quantity,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        bint venue_order_id_modified=*,
    ) except *
