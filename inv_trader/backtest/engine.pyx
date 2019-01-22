#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="engine.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False

import logging

from cpython.datetime cimport datetime, timedelta
from pandas import DataFrame
from typing import List, Dict
from logging import INFO, DEBUG

from inv_trader.core.precondition cimport Precondition
from inv_trader.backtest.data cimport BacktestDataClient
from inv_trader.backtest.execution cimport BacktestExecClient
from inv_trader.common.clock cimport LiveClock, TestClock
from inv_trader.common.logger cimport Logger
from inv_trader.enums.resolution cimport Resolution
from inv_trader.model.objects cimport Symbol, Instrument
from inv_trader.strategy cimport TradeStrategy


cdef class BacktestConfig:
    """
    Represents a configuration for a BacktestEngine.
    """
    def __init__(self,
                 int starting_capital=1000000,
                 int slippage_ticks=0,
                 bint bypass_logging=True,
                 level_console: logging=INFO,
                 level_file: logging=DEBUG,
                 bint console_prints=False,
                 bint log_to_file=False,
                 str log_file_path='backtests/'):
        """
        Initializes a new instance of the BacktestEngine class.

        :param level_console: The minimum log level for logging messages to the console.
        :param level_console: The minimum log level for logging messages to the console.
        :param level_console: The minimum log level for logging messages to the console.
        :param level_file: The minimum log level for logging messages to the log file.
        :param console_prints: The boolean flag indicating whether log messages should print.
        :param log_to_file: The boolean flag indicating whether log messages should log to file
        :param log_file_path: The name of the log file (cannot be None if log_to_file is True).
        """
        Precondition.positive(starting_capital, 'starting_capital')
        Precondition.not_negative(slippage_ticks, 'slippage_ticks')

        self.starting_capital = starting_capital
        self.slippage_ticks = slippage_ticks
        self.bypass_logging = bypass_logging
        self.level_console = level_console
        self.level_file = level_file
        self.console_prints = console_prints
        self.log_to_file = log_to_file
        self.log_file_path = log_file_path


cdef class BacktestEngine:
    """
    Provides a backtest engine to run a portfolio of strategies inside a Trader
    on historical data.
    """

    def __init__(self,
                 list instruments: List[Instrument],
                 dict data_ticks: Dict[Symbol, DataFrame],
                 dict data_bars_bid: Dict[Symbol, Dict[Resolution, DataFrame]],
                 dict data_bars_ask: Dict[Symbol, Dict[Resolution, DataFrame]],
                 list strategies: List[TradeStrategy],
                 BacktestConfig config=BacktestConfig()):
        """
        Initializes a new instance of the BacktestEngine class.

        :param strategies: The strategies to backtest.
        :param data_bars_bid: The historical bid market data needed for the backtest.
        :param data_bars_ask: The historical ask market data needed for the backtest.
        :param strategies: The strategies for the backtest.
        :param config: The configuration for the backtest.
        """
        Precondition.list_type(instruments, Instrument, 'instruments')
        Precondition.list_type(strategies, TradeStrategy, 'strategies')
        # Data checked in BacktestDataClient

        self.config = config
        self.clock = LiveClock()
        self.test_clock = TestClock()
        self.log = Logger()

        self.test_log = Logger(
            name='backtest',
            bypass_logging=config.bypass_logging,
            level_console=config.level_console,
            level_file=config.level_file,
            console_prints=config.console_prints,
            log_to_file=config.log_to_file,
            log_file_path=config.log_file_path,
            clock=self.test_clock)

        self.data_client = BacktestDataClient(
            instruments,
            data_ticks,
            data_bars_bid,
            data_bars_ask,
            clock=TestClock(),
            logger=self.test_log)

        self.exec_client = BacktestExecClient(
            instruments,
            data_ticks,
            self.data_client.get_minute_bid_bars(),
            self.data_client.get_minute_ask_bars(),
            data_minute_index=self.data_client.data_minute_index,
            starting_capital=config.starting_capital,
            slippage_ticks=config.slippage_ticks,
            clock=TestClock(),
            logger=self.test_log)

        self.data_minute_index = self.data_client.data_minute_index

        assert(self.data_minute_index == self.data_client.data_minute_index)
        assert(self.data_minute_index == self.exec_client.data_minute_index)

        for strategy in strategies:
            # Replace strategies clocks with test clocks
            strategy._change_clock(TestClock())
            # Replace strategies loggers with test loggers
            strategy._change_logger(self.test_log)

        self.trader = Trader(
            'Backtest',
            strategies,
            self.data_client,
            self.exec_client,
            self.test_clock)

    cpdef void run(
            self,
            datetime start,
            datetime stop,
            int time_step_mins=1):
        """
        Run the backtest.
        
        :param start: The start time for the backtest (must be >= first_timestamp and < stop).
        :param stop: The stop time for the backtest (must be <= last_timestamp and > start).
        :param time_step_mins: The time step in minutes for each test clock iterations (> 0)
        
        Note: The default time_step_mins is 1 and shouldn't need to be changed.
        """
        Precondition.true(start < stop, 'start < stop')
        Precondition.true(start >= self.data_minute_index[0], 'start >= self.first_timestamp')
        Precondition.true(stop <= self.data_minute_index[- 1], 'stop <= self.last_timestamp')
        Precondition.positive(time_step_mins, 'time_step_mins')

        cdef time_step = timedelta(minutes=time_step_mins)

        self.test_log.info(f"Running backtest from {start} to {stop} with {time_step_mins} minute time steps.")
        cdef datetime time = start
        self.test_clock.set_time(time)

        # Set all strategy clocks to the start of the backtest period
        for strategy in self.trader.strategies:
            strategy._set_time(start)  # Access of protected method ok here

        self.trader.start()

        self.data_client.set_initial_iteration(start, time_step)  # Also sets clock to start time
        self.exec_client.set_initial_iteration(start, time_step)  # Also sets clock to start time

        assert(self.data_client.iteration == self.exec_client.iteration)
        assert(self.data_client._clock.time_now() == start)  # Access of protected method ok here
        assert(self.exec_client._clock.time_now() == start)  # Access of protected method ok here

        while time < stop:
            # Iterate execution first to simulate correct order of events
            # Order fills should occur before the bar closes
            self.exec_client.iterate(time)
            for strategy in self.trader.strategies:
                strategy._iterate(time)  # Access of protected method ok here
            self.data_client.iterate(time)
            time += time_step
            self.test_clock.set_time(time)

        self.trader.stop()

    cpdef void reset(self):
        """
        Reset the backtest engine. The internal trader and all strategies are reset.
        """
        self.trader.reset()
