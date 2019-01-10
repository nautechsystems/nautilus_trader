#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="engine.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

from inv_trader.backtest.data cimport BacktestDataClient
from inv_trader.backtest.execution cimport BacktestExecClient
from inv_trader.trader cimport Trader


cdef class BacktestEngine:
    """
    Provides a backtest engine to run a trader on historical data.
    """
    cdef dict data
    cdef dict bar_builders
    cdef BacktestDataClient data_client
    cdef BacktestExecClient exec_client
    cdef Trader trader
