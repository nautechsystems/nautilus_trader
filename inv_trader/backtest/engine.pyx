#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="engine.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

import pandas as pd

from cpython.datetime cimport datetime, timedelta
from pandas import DataFrame
from typing import List, Dict

from inv_trader.backtest.data cimport BacktestDataClient
from inv_trader.backtest.execution cimport BacktestExecClient
from inv_trader.core.precondition cimport Precondition
from inv_trader.common.clock cimport TestClock
from inv_trader.enums.resolution cimport Resolution
from inv_trader.model.objects cimport Symbol, Instrument, BarType
from inv_trader.strategy cimport TradeStrategy


cdef class BacktestEngine:
    """
    Provides a backtest engine to run a trader on historical data.
    """

    def __init__(self,
                 list instruments: List[Instrument],
                 dict bar_data_bid: Dict[Symbol, Dict[Resolution, DataFrame]],
                 dict bar_data_ask: Dict[Symbol, Dict[Resolution, DataFrame]],
                 list strategies: List[TradeStrategy]):
        """
        Initializes a new instance of the BacktestEngine class.

        :param strategies: The strategies to backtest.
        :param bar_data_bid: The historical bid market data needed for the backtest.
        :param bar_data_ask: The historical ask market data needed for the backtest.
        :param strategies: The strategies for the backtest.
        """
        Precondition.list_type(instruments, Instrument, 'instruments')
        Precondition.list_type(strategies, TradeStrategy, 'strategies')
        # Data checked in BacktestDataClient

        self.backtest_clock = TestClock()
        self.data_client = BacktestDataClient(instruments,
                                              bar_data_bid,
                                              bar_data_ask)
        self.exec_client = BacktestExecClient()

        # Get first and last timestamp from bar data
        first_symbol = next(iter(bar_data_bid))
        first_resolution = next(iter(bar_data_bid[first_symbol]))
        first_dataframe = bar_data_bid[first_symbol][first_resolution]
        self.first_timestamp = pd.to_datetime(first_dataframe.index[0], utc=True)
        self.last_timestamp = pd.to_datetime(first_dataframe.index[len(first_dataframe) - 1], utc=True)

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
            self.backtest_clock)

    cpdef void run(
            self,
            datetime start,
            datetime stop,
            timedelta time_step=timedelta(minutes=1)):
        """
        Run the backtest.
        
        :param start: The start time for the backtest (must be >= first_timestamp and < stop).
        :param stop: The stop time for the backtest (must be <= last_timestamp and > start).
        :param time_step: The time step for each test clock iterations (should be default 1 minute).
        """
        Precondition.true(start < stop, 'start < stop')
        Precondition.true(start >= self.first_timestamp, 'start >= self.first_timestamp')
        Precondition.true(stop <= self.last_timestamp, 'stop <= self.last_timestamp')

        cdef datetime time = start
        self.backtest_clock.set_time(time)
        self.trader.start()

        while time < stop:
            # Iterate execution first to simulate correct order of events.
            # Order fills should occur before the bar closes.
            self.exec_client.iterate(time)
            for strategy in self.trader.strategies:
                strategy._clock.set_time(time)
            self.data_client.iterate(time)
            time += time_step
            self.backtest_clock.set_time(time)

