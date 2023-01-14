# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.throttler cimport Throttler
from nautilus_trader.core.message cimport Command
from nautilus_trader.core.message cimport Event
from nautilus_trader.execution.messages cimport CancelAllOrders
from nautilus_trader.execution.messages cimport CancelOrder
from nautilus_trader.execution.messages cimport ModifyOrder
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.execution.messages cimport TradingCommand
from nautilus_trader.model.enums_c cimport TradingState
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.list cimport OrderList
from nautilus_trader.portfolio.base cimport PortfolioFacade


cdef class RiskEngine(Component):
    cdef PortfolioFacade _portfolio
    cdef Cache _cache
    cdef dict _max_notional_per_order
    cdef Throttler _order_submit_throttler
    cdef Throttler _order_modify_throttler

    cdef readonly TradingState trading_state
    """The current trading state for the engine.\n\n:returns: `TradingState`"""
    cdef readonly bint is_bypassed
    """If the risk engine is completely bypassed.\n\n:returns: `bool`"""
    cdef readonly bint deny_modify_pending_update
    """If deny `ModifyOrder` commands when an order is in a `PENDING_UPDATE` state.\n\n:returns: `bool`"""
    cdef readonly bint debug
    """If debug mode is active (will provide extra debug logging).\n\n:returns: `bool`"""
    cdef readonly int command_count
    """The total count of commands received by the engine.\n\n:returns: `int`"""
    cdef readonly int event_count
    """The total count of events received by the engine.\n\n:returns: `int`"""

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void execute(self, Command command) except *
    cpdef void process(self, Event event) except *
    cpdef void set_trading_state(self, TradingState state) except *
    cpdef void set_max_notional_per_order(self, InstrumentId instrument_id, new_value: Decimal) except *
    cdef void _log_state(self) except *

# -- RISK SETTINGS --------------------------------------------------------------------------------

    cpdef tuple max_order_submit_rate(self)
    cpdef tuple max_order_modify_rate(self)
    cpdef dict max_notionals_per_order(self)
    cpdef object max_notional_per_order(self, InstrumentId instrument_id)

# -- ABSTRACT METHODS -----------------------------------------------------------------------------

    cpdef void _on_start(self) except *
    cpdef void _on_stop(self) except *

# -- COMMAND HANDLERS -----------------------------------------------------------------------------

    cdef void _execute_command(self, Command command) except *
    cdef void _handle_submit_order(self, SubmitOrder command) except *
    cdef void _handle_submit_order_list(self, SubmitOrderList command) except *
    cdef void _handle_modify_order(self, ModifyOrder command) except *
    cdef void _handle_cancel_order(self, CancelOrder command) except *
    cdef void _handle_cancel_all_orders(self, CancelAllOrders command) except *

# -- PRE-TRADE CHECKS -----------------------------------------------------------------------------

    cdef bint _check_order_id(self, Order order) except *
    cdef bint _check_order(self, Instrument instrument, Order order) except *
    cdef bint _check_order_price(self, Instrument instrument, Order order) except *
    cdef bint _check_order_quantity(self, Instrument instrument, Order order) except *
    cdef bint _check_orders_risk(self, Instrument instrument, list orders) except *
    cdef str _check_price(self, Instrument instrument, Price price)
    cdef str _check_quantity(self, Instrument instrument, Quantity quantity)

# -- DENIALS --------------------------------------------------------------------------------------

    cdef void _deny_command(self, TradingCommand command, str reason) except *
    cpdef void _deny_new_order(self, TradingCommand command) except *
    cdef void _deny_order(self, Order order, str reason) except *
    cdef void _deny_order_list(self, OrderList order_list, str reason) except *

# -- EGRESS ---------------------------------------------------------------------------------------

    cdef void _execution_gateway(self, Instrument instrument, TradingCommand command) except *
    cpdef void _send_to_execution(self, TradingCommand command) except *
    cpdef void _send_to_emulator(self, TradingCommand command) except *

# -- EVENT HANDLERS -------------------------------------------------------------------------------

    cpdef void _handle_event(self, Event event) except *
