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

from decimal import Decimal

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.component cimport Throttler
from nautilus_trader.core.message cimport Command
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.rust.model cimport TradingState
from nautilus_trader.execution.messages cimport CancelAllOrders
from nautilus_trader.execution.messages cimport CancelOrder
from nautilus_trader.execution.messages cimport ModifyOrder
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.execution.messages cimport TradingCommand
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.list cimport OrderList
from nautilus_trader.portfolio.base cimport PortfolioFacade


cdef class RiskEngine(Component):
    cdef readonly PortfolioFacade _portfolio
    cdef readonly Cache _cache
    cdef readonly dict _max_notional_per_order
    cdef readonly Throttler _order_submit_throttler
    cdef readonly Throttler _order_modify_throttler

    cdef readonly TradingState trading_state
    """The current trading state for the engine.\n\n:returns: `TradingState`"""
    cdef readonly bint is_bypassed
    """If the risk engine is completely bypassed.\n\n:returns: `bool`"""
    cdef readonly bint debug
    """If debug mode is active (will provide extra debug logging).\n\n:returns: `bool`"""
    cdef readonly int command_count
    """The total count of commands received by the engine.\n\n:returns: `int`"""
    cdef readonly int event_count
    """The total count of events received by the engine.\n\n:returns: `int`"""

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void execute(self, Command command)
    cpdef void process(self, Event event)
    cpdef void set_trading_state(self, TradingState state)
    cpdef void set_max_notional_per_order(self, InstrumentId instrument_id, new_value: Decimal)
    cpdef void _log_state(self)

# -- RISK SETTINGS --------------------------------------------------------------------------------

    cpdef tuple max_order_submit_rate(self)
    cpdef tuple max_order_modify_rate(self)
    cpdef dict max_notionals_per_order(self)
    cpdef object max_notional_per_order(self, InstrumentId instrument_id)

# -- ABSTRACT METHODS -----------------------------------------------------------------------------

    cpdef void _on_start(self)
    cpdef void _on_stop(self)

# -- COMMAND HANDLERS -----------------------------------------------------------------------------

    cpdef void _execute_command(self, Command command)
    cpdef void _handle_submit_order(self, SubmitOrder command)
    cpdef void _handle_submit_order_list(self, SubmitOrderList command)
    cpdef void _handle_modify_order(self, ModifyOrder command)

# -- PRE-TRADE CHECKS -----------------------------------------------------------------------------

    cpdef bint _check_order(self, Instrument instrument, Order order)
    cpdef bint _check_order_price(self, Instrument instrument, Order order)
    cpdef bint _check_order_quantity(self, Instrument instrument, Order order)
    cpdef bint _check_orders_risk(self, Instrument instrument, list orders)
    cpdef str _check_price(self, Instrument instrument, Price price)
    cpdef str _check_quantity(self, Instrument instrument, Quantity quantity, bint is_quote_quantity=*)

# -- DENIALS --------------------------------------------------------------------------------------

    cpdef void _deny_command(self, TradingCommand command, str reason)
    cpdef void _deny_new_order(self, TradingCommand command)
    cpdef void _deny_modify_order(self, ModifyOrder command)
    cpdef void _deny_order(self, Order order, str reason)
    cpdef void _deny_order_list(self, OrderList order_list, str reason)
    cpdef void _reject_modify_order(self, Order order, str reason)

# -- EGRESS ---------------------------------------------------------------------------------------

    cpdef void _execution_gateway(self, Instrument instrument, TradingCommand command)
    cpdef void _send_to_execution(self, TradingCommand command)

# -- EVENT HANDLERS -------------------------------------------------------------------------------

    cpdef void _handle_event(self, Event event)
