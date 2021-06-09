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

from libc.stdint cimport int64_t

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.model.c_enums.account_type cimport AccountType
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.venue_type cimport VenueType
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.commands cimport UpdateOrder
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.events cimport Event
from nautilus_trader.model.events cimport OrderFilled
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.trading.account cimport Account


cdef class ExecutionClient:
    cdef Clock _clock
    cdef UUIDFactory _uuid_factory
    cdef LoggerAdapter _log
    cdef ExecutionEngine _engine
    cdef Account _account
    cdef dict _config

    cdef readonly ClientId id
    """The clients identifier.\n\n:returns: `ClientId`"""
    cdef readonly Venue venue
    """The clients venue identifier (if not multi-venue brokerage).\n\n:returns: `Venue` or None"""
    cdef readonly VenueType venue_type
    """The clients venue type.\n\n:returns: `VenueType`"""
    cdef readonly AccountId account_id
    """The clients account identifier.\n\n:returns: `AccountId`"""
    cdef readonly AccountType account_type
    """The clients account type.\n\n:returns: `AccountType`"""
    cdef readonly Currency base_currency
    """The clients account base currency (None for multi-currency accounts).\n\n:returns: `Currency` or None"""
    cdef readonly bint calculate_account_state
    """If the account state is calculated on order fill.\n\n:returns: `bool"""
    cdef readonly bint is_connected
    """If the client is connected.\n\n:returns: `bool`"""

    cpdef void register_account(self, Account account) except *
    cpdef Account get_account(self)

    cpdef void _set_connected(self, bint value=*) except *
    cpdef void connect(self) except *
    cpdef void disconnect(self) except *
    cpdef void reset(self) except *
    cpdef void dispose(self) except *

# -- COMMAND HANDLERS ------------------------------------------------------------------------------

    cpdef void submit_order(self, SubmitOrder command) except *
    cpdef void submit_bracket_order(self, SubmitBracketOrder command) except *
    cpdef void update_order(self, UpdateOrder command) except *
    cpdef void cancel_order(self, CancelOrder command) except *

# -- EVENT HANDLERS --------------------------------------------------------------------------------

    cpdef void generate_account_state(self, list balances, bint reported, int64_t ts_updated_ns, dict info=*) except *
    cpdef void generate_order_invalid(self, ClientOrderId client_order_id, str reason) except *
    cpdef void generate_order_submitted(self, ClientOrderId client_order_id, int64_t ts_submitted_ns) except *
    cpdef void generate_order_rejected(self, ClientOrderId client_order_id, str reason, int64_t ts_rejected_ns) except *
    cpdef void generate_order_accepted(self, ClientOrderId client_order_id, VenueOrderId venue_order_id, int64_t ts_accepted_ns) except *
    cpdef void generate_order_pending_replace(self, ClientOrderId client_order_id, VenueOrderId venue_order_id, int64_t ts_pending_ns) except *
    cpdef void generate_order_pending_cancel(self, ClientOrderId client_order_id, VenueOrderId venue_order_id, int64_t ts_pending_ns) except *
    cpdef void generate_order_update_rejected(
        self,
        ClientOrderId client_order_id,
        str response_to,
        str reason,
        int64_t ts_rejected_ns,
    ) except *
    cpdef void generate_order_cancel_rejected(
        self,
        ClientOrderId client_order_id,
        str response_to,
        str reason,
        int64_t ts_rejected_ns,
    ) except *
    cpdef void generate_order_updated(
        self,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        Quantity quantity,
        Price price,
        int64_t ts_updated_ns,
        bint venue_order_id_modified=*,
    ) except *
    cpdef void generate_order_canceled(self, ClientOrderId client_order_id, VenueOrderId venue_order_id, int64_t ts_canceled_ns) except *
    cpdef void generate_order_triggered(self, ClientOrderId client_order_id, VenueOrderId venue_order_id, int64_t ts_triggered_ns) except *
    cpdef void generate_order_expired(self, ClientOrderId client_order_id, VenueOrderId venue_order_id, int64_t ts_expired_ns) except *
    cpdef void generate_order_filled(
        self,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        ExecutionId execution_id,
        PositionId position_id,
        InstrumentId instrument_id,
        OrderSide order_side,
        Quantity last_qty,
        Price last_px,
        Currency quote_currency,
        Money commission,
        LiquiditySide liquidity_side,
        int64_t ts_filled_ns,
    ) except *

# --------------------------------------------------------------------------------------------------

    cdef void _handle_event(self, Event event) except *
    cdef list _calculate_balances(self, OrderFilled fill)
    cdef list _calculate_balance_single_currency(self, OrderFilled fill, Money pnl)
    cdef list _calculate_balance_multi_currency(self, OrderFilled fill, list pnls)
