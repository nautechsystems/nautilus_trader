#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="execution.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.common.account cimport Account
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.guid cimport GuidFactory
from nautilus_trader.common.logger cimport LoggerAdapter
from nautilus_trader.model.events cimport Event
from nautilus_trader.model.identifiers cimport StrategyId, OrderId, PositionId
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.commands cimport Command, CollateralInquiry
from nautilus_trader.model.commands cimport SubmitOrder, SubmitAtomicOrder, ModifyOrder, CancelOrder
from nautilus_trader.trade.portfolio cimport Portfolio
from nautilus_trader.trade.strategy cimport TradeStrategy


cdef class ExecutionClient:
    """
    The base class for all execution clients.
    """
    cdef Clock _clock
    cdef GuidFactory _guid_factory
    cdef LoggerAdapter _log
    cdef Account _account
    cdef Portfolio _portfolio
    cdef dict _registered_strategies
    cdef dict _order_strategy_index
    cdef dict _order_book
    cdef dict _orders_active
    cdef dict _orders_completed

    cdef readonly int command_count
    cdef readonly int event_count

    cpdef datetime time_now(self)
    cpdef Account get_account(self)
    cpdef Portfolio get_portfolio(self)
    cpdef void connect(self)
    cpdef void disconnect(self)
    cpdef void check_residuals(self)
    cpdef void execute_command(self, Command command)
    cpdef void handle_event(self, Event event)
    cpdef void register_strategy(self, TradeStrategy strategy)
    cpdef void deregister_strategy(self, TradeStrategy strategy)
    cpdef Order get_order(self, OrderId order_id)
    cpdef dict get_orders_all(self)
    cpdef dict get_orders_active_all(self)
    cpdef dict get_orders_completed_all(self)
    cpdef dict get_orders(self, StrategyId strategy_id)
    cpdef dict get_orders_active(self, StrategyId strategy_id)
    cpdef dict get_orders_completed(self, StrategyId strategy_id)
    cpdef bint is_order_exists(self, OrderId order_id)
    cpdef bint is_order_active(self, OrderId order_id)
    cpdef bint is_order_complete(self, OrderId order_id)

    cdef void _execute_command(self, Command command)
    cdef void _handle_event(self, Event event)
    cdef void _register_order(self, Order order, StrategyId strategy_id, PositionId position_id)

# -- ABSTRACT METHODS ---------------------------------------------------------------------------- #
    cdef void _collateral_inquiry(self, CollateralInquiry command)
    cdef void _submit_order(self, SubmitOrder command)
    cdef void _submit_atomic_order(self, SubmitAtomicOrder command)
    cdef void _modify_order(self, ModifyOrder command)
    cdef void _cancel_order(self, CancelOrder command)
    cdef void _check_residuals(self)
    cdef void _reset(self)
