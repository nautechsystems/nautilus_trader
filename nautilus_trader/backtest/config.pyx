# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

"""
The `BacktestEngine` is highly configurable, with options being held in a
dedicated config class.
"""

from nautilus_trader.common.logging cimport LogLevel
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.currency cimport USD
from nautilus_trader.model.objects cimport Money


cdef class BacktestConfig:
    """
    Provides a configuration for a `BacktestEngine`.
    """
    def __init__(
            self,
            int tick_capacity=1000,
            int bar_capacity=1000,
            str exec_db_type not None="in-memory",
            bint exec_db_flush=True,
            bint frozen_account=False,
            bint generate_position_ids=True,
            int starting_capital=1000000,
            Currency account_currency not None=USD,
            str short_term_interest_csv_path not None="default",
            bint bypass_logging=False,
            int level_console=LogLevel.INFO,
            int level_file=LogLevel.DEBUG,
            int level_store=LogLevel.WARNING,
            bint console_prints=True,
            bint log_thread=False,
            bint log_to_file=False,
            str log_file_path not None="backtests/",
    ):
        """
        Initialize a new instance of the BacktestConfig class.

        Parameters
        ----------
        tick_capacity : int
            The length for the data engines internal ticks deque (> 0).
        bar_capacity : int
            The length for the data engines internal bars deque (> 0).
        exec_db_type : str
            The type for the execution cache (can be the default 'in-memory' or redis).
        exec_db_flush : bool
            If the execution cache should be flushed on each run.
        frozen_account : bool
            If the account should be frozen for testing (no pnl applied).
        generate_position_ids : bool
            If the simulated market should generate position identifiers.
        starting_capital : int
            The starting account capital (> 0).
        account_currency : Currency
            The currency for the account.
        short_term_interest_csv_path : str
            The path for the short term interest csv data (default='default').
        bypass_logging : bool
            If logging should be bypassed.
        level_console : int
            The minimum log level for logging messages to the console.
        level_file  : int
            The minimum log level for logging messages to the log file.
        level_store : int
            The minimum log level for storing log messages in memory.
        console_prints : bool
            The boolean flag indicating whether log messages should print.
        log_thread : bool
            The boolean flag indicating whether log messages should log the thread.
        log_to_file : bool
            The boolean flag indicating whether log messages should log to file.
        log_file_path : str
            The name of the log file (cannot be None if log_to_file is True).

        Raises
        ------
        ValueError
            If tick_capacity is not positive (> 0).
        ValueError
            If bar_capacity is not positive (> 0).
        ValueError
            If starting_capital is not positive (> 0).
        ValueError
            If commission_rate is negative (< 0).

        """
        Condition.positive_int(tick_capacity, "tick_capacity")
        Condition.positive_int(bar_capacity, "bar_capacity")
        Condition.valid_string(exec_db_type, "exec_db_type")
        Condition.positive_int(starting_capital, "starting_capital")
        Condition.valid_string(short_term_interest_csv_path, "short_term_interest_csv_path")

        self.tick_capacity = tick_capacity
        self.bar_capacity = bar_capacity
        self.exec_db_type = exec_db_type
        self.exec_db_flush = exec_db_flush
        self.frozen_account = frozen_account
        self.starting_capital = Money(starting_capital, account_currency)
        self.account_currency = account_currency
        self.short_term_interest_csv_path = short_term_interest_csv_path
        self.bypass_logging = bypass_logging
        self.level_console = level_console
        self.level_file = level_file
        self.level_store = level_store
        self.console_prints = console_prints
        self.log_thread = log_thread
        self.log_to_file = log_to_file
        self.log_file_path = log_file_path
