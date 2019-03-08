#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="execution.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

from cpython.datetime cimport datetime, timedelta

from inv_trader.common.execution cimport ExecutionClient
from inv_trader.model.objects cimport Symbol, Price, Money
from inv_trader.model.order cimport Order, OrderEvent


cdef class BacktestExecClient(ExecutionClient):
    """
    Provides an execution client for the BacktestEngine.
    """
    cdef object _message_bus

    cdef readonly dict instruments
    cdef readonly dict data_ticks
    cdef readonly dict data_bars_bid
    cdef readonly dict data_bars_ask
    cdef readonly list data_minute_index
    cdef readonly int iteration
    cdef readonly int day_number
    cdef readonly int leverage
    cdef readonly Money account_capital
    cdef readonly Money account_cash_start_day
    cdef readonly Money account_cash_activity_day
    cdef readonly dict slippage_index
    cdef readonly dict working_orders
    cdef readonly dict atomic_orders

    cpdef void set_initial_iteration(self, datetime to_time, timedelta time_step)
    cpdef void iterate(self)
    cpdef void process(self)

    cdef dict _prepare_minute_data(self, dict bar_data, str quote_type)
    cpdef list _convert_to_prices(self, double[:] values, int precision)
    cdef void _set_slippage_index(self, int slippage_ticks)
    cdef Price _get_highest_bid(self, Symbol symbol)
    cdef Price _get_lowest_bid(self, Symbol symbol)
    cdef Price _get_closing_bid(self, Symbol symbol)
    cdef Price _get_highest_ask(self, Symbol symbol)
    cdef Price _get_lowest_ask(self, Symbol symbol)
    cdef Price _get_closing_ask(self, Symbol symbol)
    cdef void _accept_order(self, Order order)
    cdef void _reject_order(self, Order order, str reason)
    cdef void _reject_modify_order(self, Order order, str reason)
    cdef void _expire_order(self, Order order)
    cdef void _work_order(self, Order order)
    cdef void _fill_order(self, Order order, Price fill_price)
    cdef void _adjust_account(self, OrderEvent event)
