#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="engine.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

from typing import Dict

from cpython.datetime cimport datetime


from inv_trader.backtest.data cimport BacktestDataClient
from inv_trader.backtest.execution cimport BacktestExecClient
from inv_trader.core.precondition cimport Precondition
from inv_trader.common.clock cimport TestClock
from inv_trader.model.objects cimport Symbol, Instrument
from inv_trader.strategy cimport TradeStrategy
from inv_trader.tools cimport BarBuilder


cdef class BacktestEngine:
    """
    Provides a backtest engine to run a trader on historical data.
    """

    def __init__(self,
                 list instruments,
                 dict data,
                 list strategies):
        """
        Initializes a new instance of the BacktestEngine class.

        :param strategies: The strategies to backtest.
        :param data: The historical market data needed for the backtest.
        """
        Precondition.list_type(instruments, Instrument, 'instruments')
        Precondition.list_type(strategies, TradeStrategy, 'strategies')

        # Assert data is the same shape



        self.data = data
        self.data_client = BacktestDataClient(instruments, data)
        self.exec_client = BacktestExecClient()

        # Convert instruments list to dictionary indexed by symbol
        instruments_dict = {}  # type: Dict[Symbol, Instrument]
        for instrument in instruments:
            instruments_dict[instrument.symbol] = instrument

        for symbol, instrument in instruments_dict:
            self.bar_builders[symbol] = BarBuilder(decimal_precision=instrument.tick_decimals)

        # Add instruments to data client
        self.data_client._instruments = instruments_dict

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
        # Pull bar handlers out of data client

        cdef dict bar_handlers = {}

        cdef datetime time = self.trader._clock.unix_epoch()
        cdef int row_n = 0

        for symbol, data in self.data.items():
            time = data.index[row_n]
            bar = self.bar_builders[symbol]._build_bar(data.iloc[row_n])

            for handler in bar_handlers[symbol]:
                handler(bar)

            self.trader._clock.set_time(time)

            for strategy in self.trader.strategies:
                strategy._clock.set_time(time)

            row_n += 1

