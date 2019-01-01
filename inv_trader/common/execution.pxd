#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="execution.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

from inv_trader.model.events cimport Event
from inv_trader.model.identifiers cimport GUID
from inv_trader.model.order cimport Order


cdef class ExecutionClient(object):
    cdef object _registered_strategies
    cdef object _order_index

    cdef readonly object log
    cdef readonly object account

    cpdef void register_strategy(self, strategy)

    cpdef void connect(self)

    cpdef void disconnect(self)

    cpdef void collateral_inquiry(self)

    cpdef void submit_order(self, Order order, GUID strategy_id)

    cpdef void cancel_order(self, Order order, str cancel_reason)

    cpdef void modify_order(self, Order order, new_price)

    cpdef void _register_order(self, Order order, GUID strategy_id)

    cpdef void _on_event(self, Event event)
