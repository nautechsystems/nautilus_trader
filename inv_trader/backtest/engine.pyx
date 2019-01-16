#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="engine.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

import logging
import pandas as pd

from cpython.datetime cimport datetime, timedelta
from pandas import DataFrame
from typing import List, Dict
from logging import INFO, DEBUG

from inv_trader.backtest.data cimport BacktestDataClient
from inv_trader.backtest.execution cimport BacktestExecClient
from inv_trader.core.precondition cimport Precondition
from inv_trader.common.clock cimport TestClock
from inv_trader.common.logger cimport Logger
from inv_trader.enums.resolution cimport Resolution
from inv_trader.model.objects cimport Symbol, Instrument
from inv_trader.strategy cimport TradeStrategy


cdef class BacktestConfig:
    """
    Represents a configuration for a BacktestEngine.
    """
    def __init__(self,
                 level_console: logging=INFO,
                 level_file: logging=DEBUG,
                 bint console_prints=False,
                 bint log_to_file=False,
                 str log_file_path='backtests/'):
        """
        Initializes a new instance of the BacktestEngine class.

        :param level_console: The minimum log level for logging messages to the console.
        :param level_file: The minimum log level for logging messages to the log file.
        :param console_prints: The boolean flag indicating whether log messages should print.
        :param log_to_file: The boolean flag indicating whether log messages should log to file
        :param log_file_path: The name of the log file (cannot be None if log_to_file is True).
        """
        self.level_console = level_console
        self.level_file = level_file
        self.console_prints = console_prints
        self.log_to_file = log_to_file
        self.log_file_path = log_file_path


cdef class BacktestEngine:
    """
    Provides a backtest engine to run a trader on historical data.
    """

    def __init__(self,
                 list instruments: List[Instrument],
                 dict tick_data: Dict[Symbol, DataFrame],
                 dict bar_data_bid: Dict[Symbol, Dict[Resolution, DataFrame]],
                 dict bar_data_ask: Dict[Symbol, Dict[Resolution, DataFrame]],
                 list strategies: List[TradeStrategy],
                 BacktestConfig config=BacktestConfig()):
        """
        Initializes a new instance of the BacktestEngine class.

        :param strategies: The strategies to backtest.
        :param bar_data_bid: The historical bid market data needed for the backtest.
        :param bar_data_ask: The historical ask market data needed for the backtest.
        :param strategies: The strategies for the backtest.
        :param config: The configuration for the backtest.
        """
        Precondition.list_type(instruments, Instrument, 'instruments')
        Precondition.list_type(strategies, TradeStrategy, 'strategies')
        # Data checked in BacktestDataClient

        self.clock = TestClock()
        self.log = Logger(
            name='backtest',
            level_console=config.level_console,
            level_file=config.level_file,
            console_prints=config.console_prints,
            log_to_file=config.log_to_file,
            log_file_path=config.log_file_path,
            clock=self.clock)

        self.data_client = BacktestDataClient(
            instruments,
            tick_data,
            bar_data_bid,
            bar_data_ask,
            clock=TestClock(),
            logger=self.log)
        self.exec_client = BacktestExecClient(
            tick_data,
            bar_data_bid,
            bar_data_ask,
            clock=TestClock(),
            logger=self.log)

        # Get first and last timestamp from bar data
        first_symbol = next(iter(bar_data_bid))
        first_resolution = next(iter(bar_data_bid[first_symbol]))
        first_dataframe = bar_data_bid[first_symbol][first_resolution]
        self.first_timestamp = pd.to_datetime(first_dataframe.index[0], utc=True)
        self.last_timestamp = pd.to_datetime(first_dataframe.index[len(first_dataframe) - 1], utc=True)

        # Replace strategies internal clocks with test clocks
        for strategy in strategies:
            strategy._change_clock(TestClock())
            strategy._change_logger(self.log)

        self.trader = Trader(
            'Backtest',
            strategies,
            self.data_client,
            self.exec_client,
            self.clock)

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

        self.log.info(f"Running backtest from {start} to {stop} with {timedelta} steps.")
        cdef datetime time = start
        self.clock.set_time(time)

        # Set all strategy clocks to the start of the backtest period.
        for strategy in self.trader.strategies:
            strategy._set_time(start)

        self.trader.start()

        while time < stop:
            # Iterate execution first to simulate correct order of events.
            # Order fills should occur before the bar closes.
            self.exec_client.iterate(time)
            for strategy in self.trader.strategies:
                strategy._iterate(time)
            self.data_client.iterate(time)
            time += time_step
            self.clock.set_time(time)

        self.trader.stop()
        self.trader.reset()
