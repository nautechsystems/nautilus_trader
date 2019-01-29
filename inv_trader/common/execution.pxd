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

from inv_trader.common.clock cimport Clock
from inv_trader.common.guid cimport GuidFactory
from inv_trader.common.logger cimport LoggerAdapter
from inv_trader.model.account cimport Account
from inv_trader.model.events cimport Event
from inv_trader.model.objects cimport Price
from inv_trader.model.identifiers cimport GUID, PositionId
from inv_trader.model.order cimport Order
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
    cdef dict _registered_strategies
    cdef dict _order_strategy_index
    cdef dict _order_book

    cpdef datetime time_now(self)
    cpdef Account get_account(self)
    cpdef Portfolio get_portfolio(self)
    cpdef void register_strategy(self, TradeStrategy strategy)
    cpdef void connect(self)
    cpdef void disconnect(self)
    cpdef void collateral_inquiry(self)
    cpdef void submit_order(self, Order order, PositionId position_id, GUID strategy_id)
    cpdef void cancel_order(self, Order order, str cancel_reason)
    cpdef void modify_order(self, Order order, Price new_price)

    cdef void _register_order(self, Order order, PositionId position_id, GUID strategy_id)
    cdef void _on_event(self, Event event)
