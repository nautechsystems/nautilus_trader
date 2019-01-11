#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="engine.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

from cpython.datetime cimport datetime
from pandas import DataFrame
from typing import List, Dict

from inv_trader.backtest.data cimport BacktestDataClient
from inv_trader.backtest.execution cimport BacktestExecClient
from inv_trader.core.precondition cimport Precondition
from inv_trader.common.clock cimport TestClock
from inv_trader.model.objects cimport Instrument, BarType
from inv_trader.strategy cimport TradeStrategy


cdef class BacktestEngine:
    """
    Provides a backtest engine to run a trader on historical data.
    """

    def __init__(self,
                 list instruments: List[Instrument],
                 dict data: Dict[BarType, DataFrame],
                 list strategies: List[TradeStrategy]):
        """
        Initializes a new instance of the BacktestEngine class.

        :param strategies: The strategies to backtest.
        :param data: The historical market data needed for the backtest.
        """
        Precondition.list_type(instruments, Instrument, 'instruments')
        Precondition.dict_types(data, BarType, DataFrame, 'data')
        Precondition.list_type(strategies, TradeStrategy, 'strategies')

        self.data_client = BacktestDataClient(instruments, data)
        self.exec_client = BacktestExecClient()

        # Replace strategies internal clocks with test clocks
        for strategy in strategies:
            strategy._change_clock(TestClock())
            self.data_client.register_strategy(strategy)
            self.exec_client.register_strategy(strategy)

        self.trader = Trader(
            'Backtest',
            strategies,
            self.data_client,
            self.exec_client,
            clock=TestClock())

    cpdef void run(self):
        """
        Run the backtest.
        """
        cdef datetime time = self.trader._clock.unix_epoch()

        self.data_client.iterate()
