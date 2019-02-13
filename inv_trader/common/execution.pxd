#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="execution.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from cpython.datetime cimport datetime

from inv_trader.common.account cimport Account
from inv_trader.common.clock cimport Clock
from inv_trader.common.guid cimport GuidFactory
from inv_trader.common.logger cimport LoggerAdapter
from inv_trader.model.events cimport Event
from inv_trader.model.identifiers cimport GUID, OrderId, PositionId
from inv_trader.model.order cimport Order
from inv_trader.commands cimport Command, CollateralInquiry
from inv_trader.commands cimport SubmitOrder, SubmitAtomicOrder, ModifyOrder, CancelOrder
from inv_trader.portfolio.portfolio cimport Portfolio
from inv_trader.strategy cimport TradeStrategy


cdef class ExecutionClient:
    """
    The abstract base class for all execution clients.
    """
    cdef Clock _clock
    cdef GuidFactory _guid_factory
    cdef LoggerAdapter _log
    cdef Account _account
    cdef Portfolio _portfolio
    cdef object _queue
    cdef dict _registered_strategies
    cdef dict _order_strategy_index
    cdef dict _order_book
    cdef dict _orders_active
    cdef dict _orders_completed

    cpdef datetime time_now(self)
    cpdef Account get_account(self)
    cpdef Portfolio get_portfolio(self)
    cpdef void connect(self)
    cpdef void disconnect(self)
    cpdef void register_strategy(self, TradeStrategy strategy)
    cpdef void execute_command(self, Command command)
    cpdef void handle_event(self, Event event)
    cpdef bint order_exists(self, OrderId order_id)
    cpdef Order get_order(self, OrderId order_id)
    cpdef dict get_orders_all(self)
    cpdef dict get_orders_active_all(self)
    cpdef dict get_orders_completed_all(self)
    cpdef dict get_orders(self, GUID strategy_id)
    cpdef dict get_orders_active(self, GUID strategy_id)
    cpdef dict get_orders_completed(self, GUID strategy_id)

    cdef void _execute_command(self, Command command)
    cdef void _handle_event(self, Event event)
    cdef void _register_order(self, Order order, PositionId position_id, GUID strategy_id)
    cdef void _collateral_inquiry(self, CollateralInquiry command)
    cdef void _submit_order(self, SubmitOrder command)
    cdef void _submit_atomic_order(self, SubmitAtomicOrder command)
    cdef void _modify_order(self, ModifyOrder command)
    cdef void _cancel_order(self, CancelOrder command)
