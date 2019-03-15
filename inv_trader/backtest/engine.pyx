#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="engine.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

import numpy as np
import logging
import psutil
import platform

from inv_trader.version import __version__
from cpython.datetime cimport datetime, timedelta
from scipy.stats import kurtosis, skew
from empyrical.stats import (
    annual_return,
    cum_returns_final,
    annual_volatility,
    sharpe_ratio,
    calmar_ratio,
    sortino_ratio,
    omega_ratio,
    stability_of_timeseries,
    max_drawdown,
    alpha,
    beta,
    tail_ratio)
from pandas import DataFrame
from typing import List, Dict
from logging import INFO, DEBUG

from inv_trader.core.precondition cimport Precondition
from inv_trader.backtest.data cimport BacktestDataClient
from inv_trader.backtest.execution cimport BacktestExecClient
from inv_trader.common.clock cimport LiveClock, TestClock
from inv_trader.common.guid cimport TestGuidFactory
from inv_trader.common.logger cimport TestLogger
from inv_trader.enums.resolution cimport Resolution
from inv_trader.common.account cimport Account
from inv_trader.model.objects cimport Symbol, Instrument, Money
from inv_trader.portfolio.portfolio cimport Portfolio
from inv_trader.strategy cimport TradeStrategy


cdef class BacktestConfig:
    """
    Represents a configuration for a BacktestEngine.
    """
    def __init__(self,
                 int starting_capital=1000000,
                 int slippage_ticks=0,
                 bint bypass_logging=False,
                 level_console: logging=INFO,
                 level_file: logging=DEBUG,
                 bint console_prints=True,
                 bint log_thread=False,
                 bint log_to_file=False,
                 str log_file_path='backtests/'):
        """
        Initializes a new instance of the BacktestEngine class.

        :param starting_capital: The starting capital for the engine (> 0).
        :param slippage_ticks: The slippage ticks for the engine (>= 0).
        :param bypass_logging: The flag indicating whether logging should be bypassed.
        :param level_console: The minimum log level for logging messages to the console.
        :param level_file: The minimum log level for logging messages to the log file.
        :param console_prints: The boolean flag indicating whether log messages should print.
        :param log_thread: The boolean flag indicating whether log messages should log the thread.
        :param log_to_file: The boolean flag indicating whether log messages should log to file.
        :param log_file_path: The name of the log file (cannot be None if log_to_file is True).
        :raises ValueError: If the starting capital is not positive (> 0).
        :raises ValueError: If the leverage is not positive (> 0).
        :raises ValueError: If the slippage_ticks is negative (< 0).
        """
        Precondition.positive(starting_capital, 'starting_capital')
        Precondition.not_negative(slippage_ticks, 'slippage_ticks')

        self.starting_capital = Money(starting_capital)
        self.slippage_ticks = slippage_ticks
        self.bypass_logging = bypass_logging
        self.level_console = level_console
        self.level_file = level_file
        self.console_prints = console_prints
        self.log_thread = log_thread
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
        :raises ValueError: If the instruments list contains a type other than Instrument.
        :raises ValueError: If the strategies list contains a type other than TradeStrategy.
        """
        Precondition.list_type(instruments, Instrument, 'instruments')
        Precondition.list_type(strategies, TradeStrategy, 'strategies')
        # Data checked in BacktestDataClient

        self.config = config
        self.clock = LiveClock()
        self.created_time = self.clock.time_now()

        self.test_clock = TestClock()
        self.test_clock.set_time(self.clock.time_now())
        self.test_logger = TestLogger(
            name='backtest',
            bypass_logging=config.bypass_logging,
            level_console=config.level_console,
            level_file=config.level_file,
            console_prints=config.console_prints,
            log_thread=config.log_thread,
            log_to_file=config.log_to_file,
            log_file_path=config.log_file_path,
            clock=self.test_clock)
        self.log = LoggerAdapter(component_name='BacktestEngine', logger=self.test_logger)

        self._engine_header()

        self.account = Account()
        self.portfolio = Portfolio(
            clock=self.test_clock,
            guid_factory=TestGuidFactory(),
            logger=self.test_logger)
        self.instruments = instruments
        self.data_client = BacktestDataClient(
            instruments=instruments,
            data_ticks=data_ticks,
            data_bars_bid=data_bars_bid,
            data_bars_ask=data_bars_ask,
            clock=self.test_clock,
            logger=self.test_logger)

        cdef dict minute_bars_bid = {}
        for symbol, data in data_bars_bid.items():
            minute_bars_bid[symbol] = data[Resolution.MINUTE]

        cdef dict minute_bars_ask = {}
        for symbol, data in data_bars_ask.items():
            minute_bars_ask[symbol] = data[Resolution.MINUTE]

        self.exec_client = BacktestExecClient(
            instruments=instruments,
            data_ticks=data_ticks,
            data_bars_bid=minute_bars_bid,
            data_bars_ask=minute_bars_ask,
            starting_capital=config.starting_capital,
            slippage_ticks=config.slippage_ticks,
            account=self.account,
            portfolio=self.portfolio,
            clock=self.test_clock,
            guid_factory=TestGuidFactory(),
            logger=self.test_logger)

        self.data_minute_index = self.data_client.data_minute_index

        assert(all(self.data_minute_index) == all(self.data_client.data_minute_index))
        assert(all(self.data_minute_index) == all(self.exec_client.data_minute_index))

        self.data_client.create_data_providers()

        for strategy in strategies:
            # Replace strategies clocks with test clocks
            strategy.change_clock(TestClock())  # Separate test clock to iterate independently
            # Replace strategies loggers with test loggers
            strategy.change_logger(self.test_logger)

        self.trader = Trader(
            'Backtest',
            strategies,
            self.data_client,
            self.exec_client,
            self.account,
            self.portfolio,
            self.test_clock)

        self.time_to_initialize = self.clock.get_elapsed(self.created_time)
        self.log.info(f'Initialized in {round(self.time_to_initialize, 2)}s.')

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
        :raises: ValueError: If the start datetime is not < the stop datetime.
        :raises: ValueError: If the start datetime is not >= the first index timestamp of data.
        :raises: ValueError: If the start datetime is not <= the last index timestamp of data.
        :raises: ValueError: If the time_step_mins is not positive (> 0).
        """
        Precondition.true(start < stop, 'start < stop')
        Precondition.true(start >= self.data_minute_index[0], 'start >= first_timestamp')
        Precondition.true(stop <= self.data_minute_index[len(self.data_minute_index) - 1], 'stop <= last_timestamp')
        Precondition.positive(time_step_mins, 'time_step_mins')

        cdef timedelta time_step = timedelta(minutes=time_step_mins)
        cdef datetime run_started = self.clock.time_now()
        cdef datetime time = start

        self._backtest_header(start, stop, time_step_mins)
        self.test_clock.set_time(time)

        self._change_strategy_clocks_and_loggers(self.trader.strategies)
        self.trader.start()

        self.data_client.set_initial_iteration(start, time_step)  # Also sets clock to start time
        self.exec_client.set_initial_iteration(start, time_step)  # Also sets clock to start time

        assert(self.data_client.iteration == self.exec_client.iteration)
        assert(self.data_client.time_now() == start)
        assert(self.exec_client.time_now() == start)

        while time <= stop:
            # Iterate execution first to simulate correct order of events
            # Order fills should occur before the bar closes
            self.test_clock.set_time(time)
            self.exec_client.iterate()
            for strategy in self.trader.strategies:
                strategy.iterate(time)
            self.data_client.iterate()
            self.exec_client.process()
            time += time_step

        self.trader.stop()
        self._backtest_footer(run_started, start, stop)

    cpdef void change_strategies(self, list strategies: List[TradeStrategy]):
        """
        Change strategies with the given list of trade strategies.
        
        :param strategies: The list of strategies to load into the engine.
        :raises ValueError: If the strategies list contains a type other than TradeStrategy.
        """
        Precondition.list_type(strategies, TradeStrategy, 'strategies')

        self._change_strategy_clocks_and_loggers(strategies)
        self.trader.change_strategies(strategies)

    cpdef void create_returns_tear_sheet(self):
        """
        Create a pyfolio returns tear sheet based on analyzer data from the last run.
        """
        self.trader.create_returns_tear_sheet()

    cpdef void create_full_tear_sheet(self):
        """
        Create a pyfolio full tear sheet based on analyzer data from the last run.
        """
        self.trader.create_full_tear_sheet()

    cpdef void reset(self):
        """
        Reset the backtest engine. The internal trader and all strategies are reset.
        """
        self.trader.reset()

    cpdef void dispose(self):
        """
        Dispose of the backtest engine by disposing the trader and releasing system resources.
        """
        self.trader.dispose()

    cdef void _engine_header(self):
        """
        Create a backtest engine log header.
        """
        self.log.info("#-----------------------------------------------------------------#")
        self.log.info("#------------------------ BACKTEST ENGINE ------------------------#")
        self.log.info("#-----------------------------------------------------------------#")
        self.log.info(f"Nautilus Trader (v{__version__}) for Invariance Pte. Limited.")
        self.log.info("Building engine...")

    cdef void _backtest_header(
            self,
            datetime start,
            datetime stop,
            int time_step_mins):
        """
        Create a backtest run log header.
        """
        self.log.info("#-----------------------------------------------------------------#")
        self.log.info("#------------------------ BACKTEST RUN ---------------------------#")
        self.log.info("#-----------------------------------------------------------------#")
        self.log.info(f"OS: {platform.platform()}")
        self.log.info(f"Processors: {platform.processor()}")
        self.log.info(f"RAM-Total: {round(psutil.virtual_memory()[0] / 1000000)}MB")
        self.log.info(f"RAM-Used:  {round(psutil.virtual_memory()[3] / 1000000)}MB")
        self.log.info(f"RAM-Avail: {round(psutil.virtual_memory()[1] / 1000000)}MB ({100 - psutil.virtual_memory()[2]}%)")
        self.log.info(f"Time-step: {time_step_mins} minute")
        self.log.info(f"Start datetime: {start}")
        self.log.info(f"Stop datetime:  {stop}")
        self.log.info(f"Account balance (starting): {self.config.starting_capital}")
        self.log.info("#-----------------------------------------------------#")
        self.log.info(f"Running backtest...")

    cdef void _backtest_footer(
            self,
            datetime run_started,
            datetime start,
            datetime stop):
        """
        Create a backtest log footer.
        """
        returns = self.trader.portfolio.analyzer.get_returns()

        self.log.info("#-----------------------------------------------------------------#")
        self.log.info("#--------------------- BACKTEST DIAGNOSTICS ----------------------#")
        self.log.info("#-----------------------------------------------------------------#")
        self.log.info(f"Elapsed time (initialization):{self._print_stat(self.time_to_initialize)}s")
        self.log.info(f"Elapsed time (running):{self._print_stat(self.clock.get_elapsed(run_started))}s")
        self.log.info(f"Time-step iterations: {self.exec_client.iteration}")
        self.log.info(f"Start datetime: {start}")
        self.log.info(f"Stop datetime:  {stop}")
        self.log.info(f"Account balance (starting):{self._print_stat(self.config.starting_capital.value)}")
        self.log.info(f"Account balance (ending):  {self._print_stat(self.account.cash_balance.value)}")
        self.log.info(f"Commissions (total):       {self._print_stat(self.exec_client.total_commissions.value)}")
        self.log.info("")
        self.log.info("#-----------------------------------------------------------------#")
        self.log.info("#--------------------- PERFORMANCE STATISTICS --------------------#")
        self.log.info("#-----------------------------------------------------------------#")
        self.log.info(f"PNL:              {self._print_stat(float((self.account.cash_balance - self.config.starting_capital).value))}")
        self.log.info(f"PNL %:            {self._print_stat(float(((self.account.cash_balance.value - self.config.starting_capital.value) / self.config.starting_capital.value) * 100))}%")
        self.log.info(f"Annual return:    {self._print_stat(annual_return(returns=returns))}%")
        self.log.info(f"Cum returns:      {self._print_stat(cum_returns_final(returns=returns))}%")
        self.log.info(f"Max drawdown:     {self._print_stat(max_drawdown(returns=returns))}%")
        self.log.info(f"Annual vol:       {self._print_stat(annual_volatility(returns=returns))}%")
        self.log.info(f"Sharpe ratio:     {self._print_stat(sharpe_ratio(returns=returns))}")
        self.log.info(f"Calmar ratio:     {self._print_stat(calmar_ratio(returns=returns))}")
        self.log.info(f"Sortino ratio:    {self._print_stat(sortino_ratio(returns=returns))}")
        self.log.info(f"Omega ratio:      {self._print_stat(omega_ratio(returns=returns))}")
        self.log.info(f"Stability:        {self._print_stat(stability_of_timeseries(returns=returns))}")
        self.log.info(f"Returns Mean:     {self._print_stat(value=np.mean(returns), decimals=5)}")
        self.log.info(f"Returns Variance: {self._print_stat(value=np.var(returns), decimals=8)}")
        self.log.info(f"Returns Skew:     {self._print_stat(skew(returns))}")
        self.log.info(f"Returns Kurtosis: {self._print_stat(kurtosis(returns))}")
        self.log.info(f"Tail ratio:       {self._print_stat(tail_ratio(returns=returns))}")
        self.log.info(f"Alpha:            {self._print_stat(alpha(returns=returns, factor_returns=returns))}")
        self.log.info(f"Beta:             {self._print_stat(beta(returns=returns, factor_returns=returns))}")
        self.log.info("#-----------------------------------------------------------------#")

    cdef void _change_strategy_clocks_and_loggers(self, list strategies):
        """
        Replace the clocks and loggers for every strategy in the given list.
        
        :param strategies: The list of strategies.
        """
        for strategy in strategies:
            # Replace the strategies clock with a test clock
            strategy.change_clock(TestClock())  # Separate test clock to iterate independently
            # Replace the strategies logger with the engines test logger
            strategy.change_logger(self.test_logger)

    cdef str _print_stat(self, value, int decimals=2):
        """
        Print the given value rounded to the given decimals with signed formatting.
        :param value: The value to print.
        :param decimals: The decimal precision for the value rounding.
        :return: str.
        """
        cdef str string_value = f'{value:.{decimals}f}'
        if not string_value.startswith('-'):
            string_value = ' ' + string_value
        return string_value
