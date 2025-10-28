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

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.component cimport TimeEvent
from nautilus_trader.common.generators cimport PositionIdGenerator
from nautilus_trader.core.message cimport Command
from nautilus_trader.core.rust.model cimport OmsType
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.execution.client cimport ExecutionClient
from nautilus_trader.execution.messages cimport BatchCancelOrders
from nautilus_trader.execution.messages cimport CancelAllOrders
from nautilus_trader.execution.messages cimport CancelOrder
from nautilus_trader.execution.messages cimport ModifyOrder
from nautilus_trader.execution.messages cimport QueryAccount
from nautilus_trader.execution.messages cimport QueryOrder
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.model.events.order cimport OrderEvent
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.events.position cimport PositionEvent
from nautilus_trader.model.identifiers cimport ClientId
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
    cdef readonly set[ClientId] _external_clients
    cdef readonly dict[ClientId, ExecutionClient] _clients
    cdef readonly dict[Venue, ExecutionClient] _routing_map
    cdef readonly dict[StrategyId, OmsType] _oms_overrides
    cdef readonly dict[InstrumentId, StrategyId] _external_order_claims
    cdef readonly PositionIdGenerator _pos_id_generator
    cdef readonly str snapshot_positions_timer_name
    cdef list[PositionEvent] _pending_position_events

    cdef readonly dict[StrategyId, str] _topic_cache_order_events
    cdef readonly dict[StrategyId, str] _topic_cache_position_events
    cdef readonly dict[InstrumentId, str] _topic_cache_fill_events
    cdef readonly dict[ClientId, str] _topic_cache_commands

    cdef readonly bint debug
    """If debug mode is active (will provide extra debug logging).\n\n:returns: `bool`"""
    cdef readonly bint convert_quote_qty_to_base
    """If quote-denominated order quantities should be converted to base units before submission.\n\n:returns: `bool`"""
    cdef readonly bint manage_own_order_books
    """If the execution engine should maintain own order books based on commands and events.\n\n:returns: `bool`"""
    cdef readonly bint snapshot_orders
    """If order state snapshots should be persisted.\n\n:returns: `bool`"""
    cdef readonly bint snapshot_positions
    """If position state snapshots should be persisted.\n\n:returns: `bool`"""
    cdef readonly double snapshot_positions_interval_secs
    """The interval (seconds) at which additional position state snapshots are persisted.\n\n:returns: `double`"""
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
    cpdef set[ClientId] get_external_client_ids(self)
    cpdef StrategyId get_external_order_claim(self, InstrumentId instrument_id)
    cpdef set[InstrumentId] get_external_order_claims_instruments(self)
    cpdef set[ExecutionClient] get_clients_for_orders(self, list[Order] orders)
    cpdef void set_manage_own_order_books(self, bint value)
    cpdef void set_convert_quote_qty_to_base(self, bint value)

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

    cdef str _get_order_events_topic(self, StrategyId strategy_id)
    cdef str _get_position_events_topic(self, StrategyId strategy_id)
    cdef str _get_fill_events_topic(self, InstrumentId instrument_id)
    cdef str _get_commands_topic(self, ClientId client_id)

    cpdef void _set_position_id_counts(self)
    cpdef Price _last_px_for_conversion(self, InstrumentId instrument_id, OrderSide order_side)
    cpdef void _set_order_base_qty(self, Order order, Quantity base_qty)
    cpdef void _deny_order(self, Order order, str reason)
    cpdef object _get_or_init_own_order_book(self, InstrumentId instrument_id)
    cpdef void _add_own_book_order(self, Order order)

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void stop_clients(self)
    cpdef void load_cache(self)
    cpdef void execute(self, Command command)
    cpdef void process(self, OrderEvent event)
    cpdef void flush_db(self)

# -- COMMAND HANDLERS -----------------------------------------------------------------------------

    cpdef void _execute_command(self, Command command)
    cpdef void _handle_submit_order(self, ExecutionClient client, SubmitOrder command)
    cpdef void _handle_submit_order_list(self, ExecutionClient client, SubmitOrderList command)
    cpdef void _handle_modify_order(self, ExecutionClient client, ModifyOrder command)
    cpdef void _handle_cancel_order(self, ExecutionClient client, CancelOrder command)
    cpdef void _handle_cancel_all_orders(self, ExecutionClient client, CancelAllOrders command)
    cpdef void _handle_batch_cancel_orders(self, ExecutionClient client, BatchCancelOrders command)
    cpdef void _handle_query_account(self, ExecutionClient client, QueryAccount command)
    cpdef void _handle_query_order(self, ExecutionClient client, QueryOrder command)

# -- EVENT HANDLERS -------------------------------------------------------------------------------

    cpdef void _handle_event(self, OrderEvent event)
    cpdef OmsType _determine_oms_type(self, OrderFilled fill)
    cpdef void _determine_position_id(self, OrderFilled fill, OmsType oms_type, Order order=*)
    cpdef PositionId _determine_hedging_position_id(self, OrderFilled fill, Order order=*)
    cpdef PositionId _determine_netting_position_id(self, OrderFilled fill)
    cpdef void _apply_event_to_order(self, Order order, OrderEvent event)
    cpdef void _handle_order_fill(self, Order order, OrderFilled fill, OmsType oms_type)
    cdef bint _is_leg_fill(self, OrderFilled fill)
    cdef void _handle_position_update(self, Instrument instrument, OrderFilled fill, OmsType oms_type)
    cpdef void _handle_leg_fill_without_order(self, OrderFilled fill)
    cpdef Position _open_position(self, Instrument instrument, Position position, OrderFilled fill, OmsType oms_type)
    cpdef void _update_position(self, Instrument instrument, Position position, OrderFilled fill, OmsType oms_type)
    cpdef bint _will_flip_position(self, Position position, OrderFilled fill)
    cpdef void _flip_position(self, Instrument instrument, Position position, OrderFilled fill, OmsType oms_type)
    cpdef void _create_order_state_snapshot(self, Order order)
    cpdef void _create_position_state_snapshot(self, Position position, bint open_only)
    cpdef void _snapshot_open_position_states(self, TimeEvent event)
