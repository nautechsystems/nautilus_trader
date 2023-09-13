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

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.generators cimport PositionIdGenerator
from nautilus_trader.execution.algorithm cimport ExecAlgorithm
from nautilus_trader.execution.client cimport ExecutionClient
from nautilus_trader.execution.messages cimport BatchCancelOrders
from nautilus_trader.execution.messages cimport CancelAllOrders
from nautilus_trader.execution.messages cimport CancelOrder
from nautilus_trader.execution.messages cimport ModifyOrder
from nautilus_trader.execution.messages cimport QueryOrder
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.execution.messages cimport TradingCommand
from nautilus_trader.model.enums_c cimport OmsType
from nautilus_trader.model.enums_c cimport OrderSide
from nautilus_trader.model.events.order cimport OrderEvent
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.position cimport Position
from nautilus_trader.trading.strategy cimport Strategy


cdef class ExecutionEngine(Component):
    cdef readonly Cache _cache
    cdef readonly ExecutionClient _default_client
    cdef readonly PositionIdGenerator _pos_id_generator
    cdef readonly dict _clients
    cdef readonly dict _routing_map
    cdef readonly dict _oms_overrides
    cdef readonly dict _external_order_claims

    cdef readonly bint debug
    """If debug mode is active (will provide extra debug logging).\n\n:returns: `bool`"""
    cdef readonly bint allow_cash_positions
    """If unleveraged spot/cash assets should generate positions.\n\n:returns: `bool`"""
    cdef readonly int command_count
    """The total count of commands received by the engine.\n\n:returns: `int`"""
    cdef readonly int event_count
    """The total count of events received by the engine.\n\n:returns: `int`"""
    cdef public int report_count
    """The total count of reports received by the engine.\n\n:returns: `int`"""

    cpdef int position_id_count(self, StrategyId strategy_id)
    cpdef bint check_integrity(self)
    cpdef bint check_connected(self)
    cpdef bint check_disconnected(self)
    cpdef bint check_residuals(self)
    cpdef StrategyId get_external_order_claim(self, InstrumentId instrument_id)
    cpdef set get_external_order_claims_instruments(self)

# -- REGISTRATION ---------------------------------------------------------------------------------

    cpdef void register_client(self, ExecutionClient client)
    cpdef void register_default_client(self, ExecutionClient client)
    cpdef void register_venue_routing(self, ExecutionClient client, Venue venue)
    cpdef void register_oms_type(self, Strategy strategy)
    cpdef void register_external_order_claims(self, Strategy strategy)
    cpdef void deregister_client(self, ExecutionClient client)

# -- ABSTRACT METHODS -----------------------------------------------------------------------------

    cpdef void _on_start(self)
    cpdef void _on_stop(self)

# -- INTERNAL -------------------------------------------------------------------------------------

    cpdef void _set_position_id_counts(self)
    cpdef Price _last_px_for_conversion(self, InstrumentId instrument_id, OrderSide order_side)
    cpdef void _set_order_base_qty(self, Order order, Quantity base_qty)
    cpdef void _deny_order(self, Order order, str reason)

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void load_cache(self)
    cpdef void execute(self, TradingCommand command)
    cpdef void process(self, OrderEvent event)
    cpdef void flush_db(self)

# -- COMMAND HANDLERS -----------------------------------------------------------------------------

    cpdef void _execute_command(self, TradingCommand command)
    cpdef void _handle_submit_order(self, ExecutionClient client, SubmitOrder command)
    cpdef void _handle_submit_order_list(self, ExecutionClient client, SubmitOrderList command)
    cpdef void _handle_modify_order(self, ExecutionClient client, ModifyOrder command)
    cpdef void _handle_cancel_order(self, ExecutionClient client, CancelOrder command)
    cpdef void _handle_cancel_all_orders(self, ExecutionClient client, CancelAllOrders command)
    cpdef void _handle_batch_cancel_orders(self, ExecutionClient client, BatchCancelOrders command)
    cpdef void _handle_query_order(self, ExecutionClient client, QueryOrder command)

# -- EVENT HANDLERS -------------------------------------------------------------------------------

    cpdef void _handle_event(self, OrderEvent event)
    cpdef OmsType _determine_oms_type(self, OrderFilled fill)
    cpdef void _determine_position_id(self, OrderFilled fill, OmsType oms_type)
    cpdef PositionId _determine_hedging_position_id(self, OrderFilled fill)
    cpdef PositionId _determine_netting_position_id(self, OrderFilled fill)
    cpdef void _apply_event_to_order(self, Order order, OrderEvent event)
    cpdef void _handle_order_fill(self, Order order, OrderFilled fill, OmsType oms_type)
    cpdef Position _open_position(self, Instrument instrument, Position position, OrderFilled fill, OmsType oms_type)
    cpdef void _update_position(self, Instrument instrument, Position position, OrderFilled fill, OmsType oms_type)
    cpdef bint _will_flip_position(self, Position position, OrderFilled fill)
    cpdef void _flip_position(self, Instrument instrument, Position position, OrderFilled fill, OmsType oms_type)
