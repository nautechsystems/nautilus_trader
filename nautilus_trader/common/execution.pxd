# -------------------------------------------------------------------------------------------------
# <copyright file="execution.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.guid cimport GuidFactory
from nautilus_trader.common.account cimport Account
from nautilus_trader.common.logger cimport LoggerAdapter
from nautilus_trader.model.events cimport Event, OrderEvent
from nautilus_trader.model.identifiers cimport TraderId, StrategyId, OrderId, PositionId
from nautilus_trader.model.position cimport Position
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.commands cimport (
    Command,
    AccountInquiry,
    SubmitOrder,
    SubmitAtomicOrder,
    ModifyOrder,
    CancelOrder
)
from nautilus_trader.common.portfolio cimport Portfolio
from nautilus_trader.trade.strategy cimport TradingStrategy


cdef class ExecutionDatabase:
    """
    Provides an execution database.
    """
    cpdef void store(self, Event event)


cdef class ExecutionEngine:
    """
    The base class for all execution engines.
    """
    cdef Clock _clock
    cdef GuidFactory _guid_factory
    cdef LoggerAdapter _log
    cdef ExecutionDatabase _exec_db
    cdef ExecutionClient _exec_client
    cdef Account _account
    cdef Portfolio _portfolio
    cdef dict _registered_strategies
    cdef dict _order_strategy_index
    cdef dict _order_p_index

    cdef dict _order_book
    cdef dict _position_book

    cdef dict _orders_active
    cdef dict _orders_completed
    cdef dict _positions_active
    cdef dict _positions_closed

    cdef readonly TraderId trader_id
    cdef readonly int command_count
    cdef readonly int event_count

    cpdef list registered_strategies(self)
    cpdef list registered_order_ids(self)
    cpdef list registered_position_ids(self)

# -- COMMANDS -----------------------------------------------------------------#
    cpdef void register_client(self, ExecutionClient exec_client)
    cpdef void register_strategy(self, TradingStrategy strategy)
    cpdef void deregister_strategy(self, TradingStrategy strategy)
    cpdef void execute_command(self, Command command)
    cpdef void handle_event(self, Event event)
    cpdef void check_residuals(self)
    cpdef void reset(self)

    cdef void _register_strategy(self, TradingStrategy strategy)
    cdef void _register_order(self, Order order, StrategyId strategy_id, PositionId position_id)
    cdef void _deregister_strategy(self, TradingStrategy strategy)
    cdef void _execute_command(self, Command command)
    cdef void _handle_event(self, Event event)
    cdef void _handle_order_fill(self, OrderEvent event, StrategyId strategy_id)
    cdef void _position_opened(self, Position position, StrategyId strategy_id, OrderEvent order_fill)
    cdef void _position_modified(self, Position position, StrategyId strategy_id, OrderEvent order_fill)
    cdef void _position_closed(self, Position position, StrategyId strategy_id, OrderEvent order_fill)
    cdef void _check_residuals(self)
    cdef void _reset(self)

# -- QUERIES ------------------------------------------------------------------#
    cpdef Order get_order(self, OrderId order_id)
    cpdef dict get_orders_all(self)
    cpdef dict get_orders_active_all(self)
    cpdef dict get_orders_completed_all(self)
    cpdef dict get_orders(self, StrategyId strategy_id)
    cpdef dict get_orders_active(self, StrategyId strategy_id)
    cpdef dict get_orders_completed(self, StrategyId strategy_id)
    cpdef bint does_order_exist(self, OrderId order_id)
    cpdef bint is_order_active(self, OrderId order_id)
    cpdef bint is_order_complete(self, OrderId order_id)

    cpdef Position get_position_for_order(self, OrderId order_id)
    cpdef Position get_position(self, PositionId position_id)
    cpdef dict get_positions_all(self)
    cpdef dict get_positions_active_all(self)
    cpdef dict get_positions_closed_all(self)
    cpdef dict get_positions(self, StrategyId strategy_id)
    cpdef dict get_positions_active(self, StrategyId strategy_id)
    cpdef dict get_positions_closed(self, StrategyId strategy_id)
    cpdef bint is_position_exists(self, PositionId position_id)
    cpdef bint is_position_active(self, PositionId position_id)
    cpdef bint is_position_closed(self, PositionId position_id)
    cpdef bint is_position_for_order(self, OrderId order_id)
    cpdef bint is_strategy_flat(self, StrategyId strategy_id)
    cpdef bint is_flat(self)
    cpdef int positions_count(self)
    cpdef int positions_active_count(self)
    cpdef int positions_closed_count(self)
# -----------------------------------------------------------------------------#


cdef class ExecutionClient:
    """
    The base class for all execution clients.
    """
    cdef LoggerAdapter _log
    cdef ExecutionEngine _engine

    cdef readonly int command_count
    cdef readonly int event_count

#-- ABSTRACT METHODS ----------------------------------------------------------#
    cpdef void connect(self)
    cpdef void disconnect(self)
    cpdef void dispose(self)
    cpdef void account_inquiry(self, AccountInquiry command)
    cpdef void submit_order(self, SubmitOrder command)
    cpdef void submit_atomic_order(self, SubmitAtomicOrder command)
    cpdef void modify_order(self, ModifyOrder command)
    cpdef void cancel_order(self, CancelOrder command)
    cpdef void reset(self)
#------------------------------------------------------------------------------#
    cdef void _reset(self)
