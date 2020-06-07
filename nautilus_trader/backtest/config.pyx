# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.common.logging cimport LogLevel
from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.objects cimport Money


cdef class BacktestConfig:
    """
    Provides a configuration for a BacktestEngine.
    """
    def __init__(self,
                 str exec_db_type not None='in-memory',
                 bint exec_db_flush=True,
                 bint frozen_account=False,
                 int starting_capital=1000000,
                 Currency account_currency=Currency.USD,
                 str short_term_interest_csv_path not None='default',
                 double commission_rate_bp=0.20,
                 bint bypass_logging=False,
                 int level_console=LogLevel.INFO,
                 int level_file=LogLevel.DEBUG,
                 int level_store=LogLevel.WARNING,
                 bint console_prints=True,
                 bint log_thread=False,
                 bint log_to_file=False,
                 str log_file_path not None='backtests/'):
        """
        Initializes a new instance of the BacktestConfig class.

        :param exec_db_type: The type for the execution database (can be the default 'in-memory' or redis).
        :param exec_db_flush: If the execution database should be flushed on each run.
        :param frozen_account: If the account should be frozen for testing (no pnl applied).
        :param starting_capital: The starting account capital (> 0).
        :param account_currency: The currency for the account.
        :param short_term_interest_csv_path: The path for the short term interest csv data (default='default').
        :param commission_rate_bp: The commission rate in basis points per notional transaction size.
        :param bypass_logging: If logging should be bypassed.
        :param level_console: The minimum log level for logging messages to the console.
        :param level_file: The minimum log level for logging messages to the log file.
        :param level_store: The minimum log level for storing log messages in memory.
        :param console_prints: The boolean flag indicating whether log messages should print.
        :param log_thread: The boolean flag indicating whether log messages should log the thread.
        :param log_to_file: The boolean flag indicating whether log messages should log to file.
        :param log_file_path: The name of the log file (cannot be None if log_to_file is True).
        :raises ValueError: If the starting capital is not positive (> 0).
        :raises ValueError: If the commission_rate is negative (< 0).
        """
        Condition.valid_string(exec_db_type, 'exec_db_type')
        Condition.positive_int(starting_capital, 'starting_capital')
        Condition.valid_string(short_term_interest_csv_path, 'short_term_interest_csv_path')
        Condition.not_negative(commission_rate_bp, 'commission_rate_bp')

        self.exec_db_type = exec_db_type
        self.exec_db_flush = exec_db_flush
        self.frozen_account = frozen_account
        self.starting_capital = Money(starting_capital, account_currency)
        self.account_currency = account_currency
        self.short_term_interest_csv_path = short_term_interest_csv_path
        self.commission_rate_bp = commission_rate_bp
        self.bypass_logging = bypass_logging
        self.level_console = level_console
        self.level_file = level_file
        self.level_store = level_store
        self.console_prints = console_prints
        self.log_thread = log_thread
        self.log_to_file = log_to_file
        self.log_file_path = log_file_path
