#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="config.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from logging import INFO, DEBUG

from inv_trader.core.precondition cimport Precondition
from inv_trader.enums.currency cimport Currency
from inv_trader.model.objects cimport Money


cdef class BacktestConfig:
    """
    Represents a configuration for a BacktestEngine.
    """
    def __init__(self,
                 int starting_capital=1000000,
                 Currency account_currency=Currency.USD,
                 float commission_rate_bp=0.20,
                 bint bypass_logging=False,
                 level_console=INFO,
                 level_file=DEBUG,
                 bint console_prints=True,
                 bint log_thread=False,
                 bint log_to_file=False,
                 str log_file_path='backtests/'):
        """
        Initializes a new instance of the BacktestConfig class.

        :param starting_capital: The starting account capital (> 0).
        :param account_currency: The currency for the account.
        :param commission_rate_bp: The commission rate in basis points per notional transaction size.
        :param bypass_logging: The flag indicating whether logging should be bypassed.
        :param level_console: The minimum log level for logging messages to the console.
        :param level_file: The minimum log level for logging messages to the log file.
        :param console_prints: The boolean flag indicating whether log messages should print.
        :param log_thread: The boolean flag indicating whether log messages should log the thread.
        :param log_to_file: The boolean flag indicating whether log messages should log to file.
        :param log_file_path: The name of the log file (cannot be None if log_to_file is True).
        :raises ValueError: If the starting capital is not positive (> 0).
        :raises ValueError: If the commission_rate is negative (< 0).
        """
        Precondition.positive(starting_capital, 'starting_capital')
        Precondition.not_negative(commission_rate_bp, 'commission_rate_bp')

        self.starting_capital = Money(starting_capital)
        self.account_currency = account_currency
        self.commission_rate_bp = commission_rate_bp
        self.bypass_logging = bypass_logging
        self.level_console = level_console
        self.level_file = level_file
        self.console_prints = console_prints
        self.log_thread = log_thread
        self.log_to_file = log_to_file
        self.log_file_path = log_file_path
