#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="execution.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

from cpython cimport array
from cpython.datetime cimport datetime, timedelta

from inv_trader.core.decimal cimport Decimal
from inv_trader.common.execution cimport ExecutionClient
from inv_trader.model.objects cimport Symbol
from inv_trader.model.order cimport Order


cdef class BacktestExecClient(ExecutionClient):
    """
    Provides an execution client for the BacktestEngine.
    """
    cdef readonly dict instruments
    cdef readonly dict data_ticks
    cdef readonly dict data_bars_bid
    cdef readonly dict data_bars_ask
    cdef readonly list data_minute_index
    cdef readonly int iteration
    cdef readonly Decimal account_cash_start_day
    cdef readonly Decimal account_cash_activity_day
    cdef readonly dict slippage_index
    cdef readonly dict working_orders

    cpdef void set_initial_iteration(self, datetime to_time, timedelta time_step)
    cpdef void iterate(self, datetime time)

    cdef dict _prepare_minute_data(self, dict bar_data, str quote_type)
    cpdef Decimal[:] _convert_to_decimals(self, double[:] values, int precision)
    cdef void _set_slippage_index(self, int slippage_ticks)
    cdef Decimal _get_highest_bid(self, Symbol symbol)
    cdef Decimal _get_lowest_bid(self, Symbol symbol)
    cdef Decimal _get_closing_bid(self, Symbol symbol)
    cdef Decimal _get_highest_ask(self, Symbol symbol)
    cdef Decimal _get_lowest_ask(self, Symbol symbol)
    cdef Decimal _get_closing_ask(self, Symbol symbol)
    cdef void _reject_order(self, Order order, str reason)
    cdef void _reject_modify_order(self, Order order, str reason)
    cdef void _expire_order(self, Order order)
    cdef void _fill_order(self, Order order, Decimal market_price)
