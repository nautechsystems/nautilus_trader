# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.generators cimport PositionIdGenerator
from nautilus_trader.execution.client cimport ExecutionClient
from nautilus_trader.execution.reports cimport ExecutionMassStatus
from nautilus_trader.execution.reports cimport ExecutionReport
from nautilus_trader.execution.reports cimport OrderStatusReport
from nautilus_trader.execution.reports cimport TradeReport
from nautilus_trader.model.c_enums.oms_type cimport OMSType
from nautilus_trader.model.commands.trading cimport CancelAllOrders
from nautilus_trader.model.commands.trading cimport CancelOrder
from nautilus_trader.model.commands.trading cimport ModifyOrder
from nautilus_trader.model.commands.trading cimport SubmitOrder
from nautilus_trader.model.commands.trading cimport SubmitOrderList
from nautilus_trader.model.commands.trading cimport TradingCommand
from nautilus_trader.model.events.order cimport OrderEvent
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.position cimport Position
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class ExecutionEngine(Component):
    cdef Cache _cache
    cdef ExecutionClient _default_client
    cdef PositionIdGenerator _pos_id_generator
    cdef dict _clients
    cdef dict _routing_map
    cdef dict _oms_types

    cdef readonly int command_count
    """The total count of commands received by the engine.\n\n:returns: `int`"""
    cdef readonly int event_count
    """The total count of events received by the engine.\n\n:returns: `int`"""
    cdef readonly int report_count
    """The total count of reports received by the engine.\n\n:returns: `int`"""

    cpdef int position_id_count(self, StrategyId strategy_id) except *
    cpdef bint check_integrity(self) except *
    cpdef bint check_connected(self) except *
    cpdef bint check_disconnected(self) except *
    cpdef bint check_residuals(self) except *

# -- REGISTRATION ----------------------------------------------------------------------------------

    cpdef void register_client(self, ExecutionClient client) except *
    cpdef void register_default_client(self, ExecutionClient client) except *
    cpdef void register_venue_routing(self, ExecutionClient client, Venue venue) except *
    cpdef void register_oms_type(self, TradingStrategy strategy) except *
    cpdef void deregister_client(self, ExecutionClient client) except *

# -- ABSTRACT METHODS ------------------------------------------------------------------------------

    cpdef void _on_start(self) except *
    cpdef void _on_stop(self) except *

# -- INTERNAL --------------------------------------------------------------------------------------

    cdef void _set_position_id_counts(self) except *

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void load_cache(self) except *
    cpdef void execute(self, TradingCommand command) except *
    cpdef void process(self, OrderEvent event) except *
    cpdef void reconcile_report(self, ExecutionReport report) except *
    cpdef void reconcile_mass_status(self, ExecutionMassStatus report) except *
    cpdef void flush_db(self) except *

# -- COMMAND HANDLERS ------------------------------------------------------------------------------

    cdef void _execute_command(self, TradingCommand command) except *
    cdef void _handle_submit_order(self, ExecutionClient client, SubmitOrder command) except *
    cdef void _handle_submit_order_list(self, ExecutionClient client, SubmitOrderList command) except *
    cdef void _handle_modify_order(self, ExecutionClient client, ModifyOrder command) except *
    cdef void _handle_cancel_order(self, ExecutionClient client, CancelOrder command) except *
    cdef void _handle_cancel_all_orders(self, ExecutionClient client, CancelAllOrders command) except *

# -- EVENT HANDLERS --------------------------------------------------------------------------------

    cdef void _handle_event(self, OrderEvent event) except *
    cdef OMSType _confirm_oms_type(self, Venue venue, StrategyId strategy_id) except *
    cdef void _confirm_position_id(self, OrderFilled fill, OMSType oms_type) except *
    cdef void _handle_order_fill(self, OrderFilled fill, OMSType oms_type) except *
    cdef void _open_position(self, OrderFilled fill, OMSType oms_type) except *
    cdef void _update_position(self, Position position, OrderFilled fill, OMSType oms_type) except *
    cdef void _flip_position(self, Position position, OrderFilled fill, OMSType oms_type) except *

# -- RECONCILIATION --------------------------------------------------------------------------------

    cdef bint _reconcile_report(self, ExecutionReport report) except *
    cdef bint _reconcile_mass_status(self, ExecutionMassStatus report) except *
    cdef bint _reconcile_order(self, OrderStatusReport report, list trades) except *
    cdef ClientOrderId _generate_client_order_id(self)
    cdef Order _generate_external_order(self, OrderStatusReport report)
    cdef void _apply_order_rejected(self, Order order, OrderStatusReport report) except *
    cdef void _apply_order_accepted(self, Order order, OrderStatusReport report) except *
    cdef void _apply_order_triggered(self, Order order, OrderStatusReport report) except *
    cdef void _apply_order_updated(self, Order order, OrderStatusReport report) except *
    cdef void _apply_order_canceled(self, Order order, OrderStatusReport report) except *
    cdef void _apply_order_expired(self, Order order, OrderStatusReport report) except *
    cdef void _apply_order_filled(self, Order order, TradeReport trade, Instrument instrument) except *
    cdef bint _should_update(self, Order order, OrderStatusReport report) except *
