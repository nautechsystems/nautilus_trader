# -------------------------------------------------------------------------------------------------
# <copyright file="execution.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.model.events cimport Event, OrderEvent, OrderCancelReject, OrderFillEvent
from nautilus_trader.model.events cimport AccountStateEvent, PositionEvent
from nautilus_trader.model.identifiers cimport AccountId, TraderId, StrategyId
from nautilus_trader.model.identifiers cimport OrderId, PositionId, PositionIdBroker
from nautilus_trader.model.position cimport Position
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.commands cimport Command, AccountInquiry, SubmitOrder, SubmitAtomicOrder
from nautilus_trader.model.commands cimport ModifyOrder, CancelOrder
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.guid cimport GuidFactory
from nautilus_trader.common.account cimport Account
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.portfolio cimport Portfolio
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class ExecutionDatabase:
    cdef LoggerAdapter _log
    cdef dict _cached_accounts
    cdef dict _cached_orders
    cdef dict _cached_positions

    cdef readonly TraderId trader_id

# -- COMMANDS --------------------------------------------------------------------------------------
    cpdef void add_account(self, Account account) except *
    cpdef void add_order(self, Order order, StrategyId strategy_id, PositionId position_id) except *
    cpdef void add_position(self, Position position, StrategyId strategy_id) except *
    cpdef void update_account(self, Account account) except *
    cpdef void update_strategy(self, TradingStrategy strategy) except *
    cpdef void update_order(self, Order order) except *
    cpdef void update_position(self, Position position) except *
    cpdef void load_strategy(self, TradingStrategy strategy) except *
    cpdef Account load_account(self, AccountId account_id)
    cpdef Order load_order(self, OrderId order_id)
    cpdef Position load_position(self, PositionId position_id)
    cpdef void delete_strategy(self, TradingStrategy strategy) except *
    cpdef void check_residuals(self) except *
    cpdef void reset(self) except *
    cpdef void flush(self) except *
    cdef void _reset(self) except *

# -- QUERIES ---------------------------------------------------------------------------------------
    cpdef Account get_account(self, AccountId account_id)
    cpdef set get_strategy_ids(self)
    cpdef set get_order_ids(self, StrategyId strategy_id=*)
    cpdef set get_order_working_ids(self, StrategyId strategy_id=*)
    cpdef set get_order_completed_ids(self, StrategyId strategy_id=*)
    cpdef set get_position_ids(self, StrategyId strategy_id=*)
    cpdef set get_position_open_ids(self, StrategyId strategy_id=*)
    cpdef set get_position_closed_ids(self, StrategyId strategy_id=*)
    cpdef StrategyId get_strategy_for_order(self, OrderId order_id)
    cpdef StrategyId get_strategy_for_position(self, PositionId position_id)
    cpdef Order get_order(self, OrderId order_id)
    cpdef dict get_orders(self, StrategyId strategy_id=*)
    cpdef dict get_orders_working(self, StrategyId strategy_id=*)
    cpdef dict get_orders_completed(self, StrategyId strategy_id=*)
    cpdef Position get_position(self, PositionId position_id)
    cpdef Position get_position_for_order(self, OrderId order_id)
    cpdef PositionId get_position_id(self, OrderId order_id)
    cpdef PositionId get_position_id_for_broker_id(self, PositionIdBroker position_id_broker)
    cpdef dict get_positions(self, StrategyId strategy_id=*)
    cpdef dict get_positions_open(self, StrategyId strategy_id=*)
    cpdef dict get_positions_closed(self, StrategyId strategy_id=*)
    cpdef bint order_exists(self, OrderId order_id)
    cpdef bint is_order_working(self, OrderId order_id)
    cpdef bint is_order_completed(self, OrderId order_id)
    cpdef bint position_exists(self, PositionId position_id)
    cpdef bint position_exists_for_order(self, OrderId order_id)
    cpdef bint position_indexed_for_order(self, OrderId order_id)
    cpdef bint is_position_open(self, PositionId position_id)
    cpdef bint is_position_closed(self, PositionId position_id)
    cpdef int count_orders_total(self, StrategyId strategy_id=*)
    cpdef int count_orders_working(self, StrategyId strategy_id=*)
    cpdef int count_orders_completed(self, StrategyId strategy_id=*)
    cpdef int count_positions_total(self, StrategyId strategy_id=*)
    cpdef int count_positions_open(self, StrategyId strategy_id=*)
    cpdef int count_positions_closed(self, StrategyId strategy_id=*)
# -------------------------------------------------------------------------------------------------"


cdef class InMemoryExecutionDatabase(ExecutionDatabase):
    cdef set _strategies
    cdef dict _index_order_position
    cdef dict _index_order_strategy
    cdef dict _index_broker_position
    cdef dict _index_position_strategy
    cdef dict _index_position_orders
    cdef dict _index_strategy_orders
    cdef dict _index_strategy_positions
    cdef set _index_orders
    cdef set _index_orders_working
    cdef set _index_orders_completed
    cdef set _index_positions
    cdef set _index_positions_open
    cdef set _index_positions_closed


cdef class ExecutionEngine:
    cdef Clock _clock
    cdef GuidFactory _guid_factory
    cdef LoggerAdapter _log
    cdef ExecutionClient _exec_client
    cdef dict _registered_strategies

    cdef readonly TraderId trader_id
    cdef readonly AccountId account_id
    cdef readonly ExecutionDatabase database
    cdef readonly Account account
    cdef readonly Portfolio portfolio
    cdef readonly int command_count
    cdef readonly int event_count

# -- COMMANDS --------------------------------------------------------------------------------------
    cpdef void register_client(self, ExecutionClient exec_client) except *
    cpdef void register_strategy(self, TradingStrategy strategy) except *
    cpdef void deregister_strategy(self, TradingStrategy strategy) except *
    cpdef void execute_command(self, Command command) except *
    cpdef void handle_event(self, Event event) except *
    cpdef void check_residuals(self) except *
    cpdef void reset(self) except *

# -- QUERIES ---------------------------------------------------------------------------------------
    cpdef list registered_strategies(self)
    cpdef bint is_strategy_flat(self, StrategyId strategy_id)
    cpdef bint is_flat(self)

# --------------------------------------------------------------------------------------------------
    cdef void _execute_command(self, Command command) except *
    cdef void _handle_event(self, Event event) except *
    cdef void _handle_order_cancel_reject(self, OrderCancelReject event) except *
    cdef void _handle_order_event(self, OrderEvent event) except *
    cdef void _handle_order_fill(self, OrderFillEvent event) except *
    cdef void _handle_position_event(self, PositionEvent event) except *
    cdef void _handle_account_event(self, AccountStateEvent event) except *
    cdef void _position_opened(self, Position position, StrategyId strategy_id, OrderEvent event) except *
    cdef void _position_modified(self, Position position, StrategyId strategy_id, OrderEvent event) except *
    cdef void _position_closed(self, Position position, StrategyId strategy_id, OrderEvent event) except *
    cdef void _send_to_strategy(self, Event event, StrategyId strategy_id) except *
    cdef void _reset(self) except *


cdef class ExecutionClient:
    cdef LoggerAdapter _log
    cdef ExecutionEngine _exec_engine

    cdef readonly int command_count
    cdef readonly int event_count

# -- ABSTRACT METHODS ------------------------------------------------------------------------------
    cpdef void connect(self) except *
    cpdef void disconnect(self) except *
    cpdef void dispose(self) except *
    cpdef void account_inquiry(self, AccountInquiry command) except *
    cpdef void submit_order(self, SubmitOrder command) except *
    cpdef void submit_atomic_order(self, SubmitAtomicOrder command) except *
    cpdef void modify_order(self, ModifyOrder command) except *
    cpdef void cancel_order(self, CancelOrder command) except *
    cpdef void reset(self) except *
# --------------------------------------------------------------------------------------------------
    cdef void _reset(self) except *
