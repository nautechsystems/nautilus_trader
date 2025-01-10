# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from libc.stdint cimport uint64_t

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.component cimport Component
from nautilus_trader.core.rust.model cimport AccountType
from nautilus_trader.core.rust.model cimport LiquiditySide
from nautilus_trader.core.rust.model cimport OmsType
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport OrderType
from nautilus_trader.execution.messages cimport BatchCancelOrders
from nautilus_trader.execution.messages cimport CancelAllOrders
from nautilus_trader.execution.messages cimport CancelOrder
from nautilus_trader.execution.messages cimport ModifyOrder
from nautilus_trader.execution.messages cimport QueryOrder
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.model.events.account cimport AccountState
from nautilus_trader.model.events.order cimport OrderEvent
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TradeId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class ExecutionClient(Component):
    cdef readonly Cache _cache

    cdef readonly OmsType oms_type
    """The venues order management system type.\n\n:returns: `OmsType`"""
    cdef readonly Venue venue
    """The clients venue ID (if not a routing client).\n\n:returns: `Venue` or ``None``"""
    cdef readonly AccountId account_id
    """The clients account ID.\n\n:returns: `AccountId` or ``None``"""
    cdef readonly AccountType account_type
    """The clients account type.\n\n:returns: `AccountType`"""
    cdef readonly Currency base_currency
    """The clients account base currency (None for multi-currency accounts).\n\n:returns: `Currency` or ``None``"""
    cdef readonly bint is_connected
    """If the client is connected.\n\n:returns: `bool`"""

    cpdef Account get_account(self)

    cpdef void _set_connected(self, bint value=*)
    cpdef void _set_account_id(self, AccountId account_id)

# -- COMMAND HANDLERS -----------------------------------------------------------------------------

    cpdef void submit_order(self, SubmitOrder command)
    cpdef void submit_order_list(self, SubmitOrderList command)
    cpdef void modify_order(self, ModifyOrder command)
    cpdef void cancel_order(self, CancelOrder command)
    cpdef void cancel_all_orders(self, CancelAllOrders command)
    cpdef void batch_cancel_orders(self, BatchCancelOrders command)
    cpdef void query_order(self, QueryOrder command)

# -- EVENT HANDLERS -------------------------------------------------------------------------------

    cpdef void generate_account_state(
        self,
        list balances,
        list margins,
        bint reported,
        uint64_t ts_event,
        dict info=*,
    )
    cpdef void generate_order_submitted(
        self,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        uint64_t ts_event,
    )
    cpdef void generate_order_rejected(
        self,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        str reason,
        uint64_t ts_event,
    )
    cpdef void generate_order_accepted(
        self,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        uint64_t ts_event,
    )
    cpdef void generate_order_modify_rejected(
        self,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        str reason,
        uint64_t ts_event,
    )
    cpdef void generate_order_cancel_rejected(
        self,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        str reason,
        uint64_t ts_event,
    )
    cpdef void generate_order_updated(
        self,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        Quantity quantity,
        Price price,
        Price trigger_price,
        uint64_t ts_event,
        bint venue_order_id_modified=*,
    )
    cpdef void generate_order_canceled(
        self,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        uint64_t ts_event,
    )
    cpdef void generate_order_triggered(
        self,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        uint64_t ts_event,
    )
    cpdef void generate_order_expired(
        self,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        uint64_t ts_event,
    )
    cpdef void generate_order_filled(
        self,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        ClientOrderId client_order_id,
        VenueOrderId venue_order_id,
        PositionId venue_position_id,
        TradeId trade_id,
        OrderSide order_side,
        OrderType order_type,
        Quantity last_qty,
        Price last_px,
        Currency quote_currency,
        Money commission,
        LiquiditySide liquidity_side,
        uint64_t ts_event,
        dict info=*,
    )

# --------------------------------------------------------------------------------------------------

    cpdef void _send_account_state(self, AccountState account_state)
    cpdef void _send_order_event(self, OrderEvent event)
    cpdef void _send_mass_status_report(self, report)
    cpdef void _send_order_status_report(self, report)
    cpdef void _send_fill_report(self, report)
