#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="execution.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

from cpython.datetime cimport datetime

from inv_trader.core.decimal cimport Decimal
from inv_trader.common.execution cimport ExecutionClient
from inv_trader.model.order cimport Order


cdef class BacktestExecClient(ExecutionClient):
    """
    Provides an execution client for the BacktestEngine.
    """
    cdef readonly dict instruments
    cdef readonly dict tick_data
    cdef readonly dict bar_data_bid
    cdef readonly dict bar_data_ask
    cdef readonly int iteration
    cdef readonly Decimal account_cash_start_day
    cdef readonly Decimal account_cash_activity_day
    cdef readonly dict current_bids
    cdef readonly dict current_asks
    cdef readonly dict slippage_index
    cdef readonly list working_orders

    cpdef void iterate(self, datetime time)

    cdef void _set_current_market_prices(self)
    cdef void _set_slippage_index(self, int slippage_ticks)
    cdef void _reject_order(self, Order order, str reason)
    cdef void _fill_order(self, Order order, Decimal market_price)
