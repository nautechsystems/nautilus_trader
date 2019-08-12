# -------------------------------------------------------------------------------------------------
# <copyright file="logger.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import os
import sys
import traceback
import threading
import cython
import numpy as np
import scipy
import pandas as pd
import logging
import psutil
import platform

from platform import python_version
from nautilus_trader.version import __version__
from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.functions cimport format_zulu_datetime
from nautilus_trader.common.clock cimport Clock, LiveClock, TestClock


cdef str HEADER = '\033[95m'
cdef str OK_BLUE = '\033[94m'
cdef str OK_GREEN = '\033[92m'
cdef str WARNING = '\033[1;33m'
cdef str FAIL = '\033[01;31m'
cdef str ENDC = '\033[0m'
cdef str BOLD = '\033[1m'
cdef str UNDERLINE = '\033[4m'


cdef class Logger:
    """
    The abstract base class for all Loggers.
    """

    def __init__(self,
                 str name=None,
                 bint bypass_logging=False,
                 int level_console: logging=logging.INFO,
                 int level_file: logging=logging.DEBUG,
                 int level_store: logging=logging.WARNING,
                 bint console_prints=True,
                 bint log_thread=False,
                 bint log_to_file=False,
                 str log_file_path='log/',
                 Clock clock=LiveClock()):
        """
        Initializes a new instance of the Logger class.

        :param name: The name of the logger.
        :param level_console: The minimum log level for logging messages to the console.
        :param level_file: The minimum log level for logging messages to the log file.
        :param level_store: The minimum log level for storing log messages in memory.
        :param console_prints: The flag indicating whether log messages should print.
        :param log_thread: The flag indicating whether log messages should log the thread.
        :param log_to_file: The flag indicating whether log messages should log to file.
        :param log_file_path: The name of the log file (cannot be None if log_to_file is True).
        :param clock: The clock for the logger.
        :raises ValueError: If the name is not a valid string.
        :raises ValueError: If the log_file_path is not a valid string.
        """
        if name is not None:
            Condition.valid_string(name, 'name')
        else:
            name = 'tmp'

        Condition.valid_string(log_file_path, 'log_file_path')

        self.name = name
        self.bypass_logging = bypass_logging
        self.clock = clock
        self._log_level_console = level_console
        self._log_level_file = level_file
        self._log_level_store = level_store
        self._console_prints = console_prints
        self._log_thread = log_thread
        self._log_to_file = log_to_file
        self._log_file_path = log_file_path
        self._log_file = f'{self._log_file_path}{self.name}.log'
        self._log_store = []
        self._logger = logging.getLogger(name)
        self._logger.setLevel(level_file)

        # Setup log file handling
        if log_to_file:
            if not os.path.exists(log_file_path):
                # Create directory if it does not exist
                os.makedirs(log_file_path)
            self._log_file_handler = logging.FileHandler(self._log_file)
            self._logger.addHandler(self._log_file_handler)

    cpdef void change_log_file_name(self, str name):
        """
        Change the log file name.
        
        :param name: The new name of the log file.
        """
        Condition.valid_string(name, 'name')

        self._log_file = f'{self._log_file_path}{name}.log'
        self._logger.removeHandler(self._log_file_handler)
        self._log_file_handler = logging.FileHandler(self._log_file)
        self._logger.addHandler(self._log_file_handler)

    cpdef void log(self, int level, str message):
        """
        Log the given message with the given log level and timestamp.
        
        :param level: The log level for the log message.
        :param message: The message to log.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef list get_log_store(self):
        """
        Return the log store of message strings.
        
        :return: List[str].
        """
        return self._log_store

    cpdef void clear_log_store(self):
        """
        Clear the log store.
        """
        self._log_store = []

    cpdef void _debug(self, datetime timestamp, str message):
        cdef str formatted_msg = self._format_message(timestamp, 'DBG', message)
        self._log_store_handler(logging.DEBUG, formatted_msg)
        self._console_print_handler(logging.DEBUG, formatted_msg)

        if self._log_to_file:
            try:
                self._logger.debug(formatted_msg)
            except IOError as ex:
                self._console_print_handler(f"IOError: {ex}.", logging.CRITICAL)

    cpdef void _info(self, datetime timestamp, str message):
        cdef str formatted_msg = self._format_message(timestamp, 'INF', message)
        self._log_store_handler(logging.INFO, formatted_msg)
        self._console_print_handler(logging.INFO, formatted_msg)

        if self._log_to_file:
            try:
                self._logger.info(formatted_msg)
            except IOError as ex:
                self._console_print_handler(f"IOError: {ex}.", logging.CRITICAL)

    cpdef void _warning(self, datetime timestamp, str message):
        cdef str formatted_msg = self._format_message(timestamp, WARNING + 'WRN' + ENDC, WARNING + message + ENDC)
        self._log_store_handler(logging.WARNING, formatted_msg)
        self._console_print_handler(logging.WARNING, formatted_msg)

        if self._log_to_file:
            try:
                self._logger.warning(formatted_msg)
            except IOError as ex:
                self._console_print_handler(f"IOError: {ex}.", logging.CRITICAL)

    cpdef void _error(self, datetime timestamp, str message):
        cdef str formatted_msg = self._format_message(timestamp, FAIL + 'ERR' + ENDC, FAIL + message + ENDC)
        self._log_store_handler(logging.ERROR, formatted_msg)
        self._console_print_handler(logging.ERROR, formatted_msg)

        if self._log_to_file:
            try:
                self._logger.error(formatted_msg)
            except IOError as ex:
                self._console_print_handler(f"IOError: {ex}.", logging.CRITICAL)

    cpdef void _critical(self, datetime timestamp, str message):
        cdef str log_message = self._format_message(timestamp, FAIL + 'CRT' + ENDC, FAIL + message + ENDC)
        self._log_store_handler(logging.CRITICAL, log_message)
        self._console_print_handler(logging.CRITICAL, log_message)

        if self._log_to_file:
            try:
                self._logger.critical(log_message)
            except IOError as ex:
                self._console_print_handler(f"IOError: {ex}.", logging.CRITICAL)

    cdef str _format_message(self, datetime timestamp, str level, str message):
        # Return the formatted log message from the given arguments
        cdef str time = format_zulu_datetime(timestamp)
        cdef str thread = '' if self._log_thread is False else f'[{threading.current_thread().ident}]'
        return f"{BOLD}{time}{ENDC} {thread}[{level}] {message}"

    cdef void _log_store_handler(self, int level, str message):
        # Store the given log message if the given log level is >= the log_level_store
        if level >= self._log_level_store:
            self._log_store.append(message)

    cdef void _console_print_handler(self, int level, str message):
        # Print the given log message to the console if the given log level if
        # >= the log_level_console level.
        if self._console_prints and level >= self._log_level_console:
            print(message)


cdef class TestLogger(Logger):
    """
    Provides a single threaded logger for testing.
    """

    def __init__(self,
                 str name=None,
                 bint bypass_logging=False,
                 int level_console: logging=logging.DEBUG,
                 int level_file: logging=logging.DEBUG,
                 int level_store: logging=logging.WARNING,
                 bint console_prints=True,
                 bint log_thread=False,
                 bint log_to_file=False,
                 str log_file_path='log/',
                 Clock clock=TestClock()):
        """
        Initializes a new instance of the TestLogger class.

        :param name: The name of the logger.
        :param level_console: The minimum log level for logging messages to the console.
        :param level_file: The minimum log level for logging messages to the log file.
        :param level_store: The minimum log level for storing log messages in memory.
        :param console_prints: The flag indicating whether log messages should print.
        :param log_thread: The flag indicating whether log messages should log the thread.
        :param log_to_file: The flag indicating whether log messages should log to file.
        :param log_file_path: The name of the log file (cannot be None if log_to_file is True).
        :param clock: The clock for the logger.
        :raises ValueError: If the name is not a valid string.
        :raises ValueError: If the log_file_path is not a valid string.
        """
        super().__init__(name,
                         bypass_logging,
                         level_console,
                         level_file,
                         level_store,
                         console_prints,
                         log_thread,
                         log_to_file,
                         log_file_path,
                         clock)

    cpdef void log(self, int level, str message):
        """
        Log the given message with the given log level and timestamp.
        
        :param level: The log level for the log message.
        :param message: The message to log.
        """
        if level == logging.DEBUG:
            self._debug(self.clock.time_now(), message)
        elif level == logging.INFO:
            self._info(self.clock.time_now(), message)
        elif level == logging.WARNING:
            self._warning(self.clock.time_now(), message)
        elif level == logging.ERROR:
            self._error(self.clock.time_now(), message)
        elif level == logging.CRITICAL:
            self._critical(self.clock.time_now(), message)
        else:
            raise RuntimeError(f"Log level {level} not recognized")


cdef class LoggerAdapter:
    """
    Provides an adapter for a components logger.
    """

    def __init__(self,
                 str component_name=None,
                 Logger logger=None):
        """
        Initializes a new instance of the LoggerAdapter class.

        :param logger: The logger for the component.
        :param component_name: The name of the component.
        """
        if component_name is None:
            component_name = ''
        else:
            Condition.valid_string(component_name, 'component_name')

        if logger is None:
            logger = TestLogger()

        self._logger = logger

        self.component_name = component_name
        self.bypassed = logger.bypass_logging

    cpdef Logger get_logger(self):
        """
        System method. Return the encapsulated logger
        
        :return: logging.logger.
        """
        return self._logger

    cpdef void debug(self, str message):
        """
        Log the given debug message with the logger.

        :param message: The debug message to log.
        """
        if not self.bypassed:
            self._logger.log(logging.DEBUG, self._format_message(message))

    cpdef void info(self, str message):
        """
        Log the given information message with the logger.

        :param message: The information message to log.
        """
        if not self.bypassed:
            self._logger.log(logging.INFO, self._format_message(message))

    cpdef void warning(self, str message):
        """
        Log the given warning message with the logger.

        :param message: The warning message to log.
        """
        if not self.bypassed:
            self._logger.log(logging.WARNING, self._format_message(message))

    cpdef void error(self, str message):
        """
        Log the given error message with the logger.

        :param message: The error message to log.
        """
        if not self.bypassed:
            self._logger.log(logging.ERROR, self._format_message(message))

    cpdef void critical(self, str message):
        """
        Log the given critical message with the logger.

        :param message: The critical message to log.
        """
        if not self.bypassed:
            self._logger.log(logging.CRITICAL, self._format_message(message))

    cpdef void exception(self, ex):
        """
        Log the given exception including stack trace information.
        
        :param ex: The exception to log.
        """
        cdef str ex_string = f'{type(ex).__name__}({ex})\n'
        exc_type, exc_value, exc_traceback = sys.exc_info()
        stack_trace = traceback.format_exception(exc_type, exc_value, exc_traceback)

        cdef str stack_trace_lines = ''
        cdef str line
        for line in stack_trace[:len(stack_trace) - 1]:
            stack_trace_lines += line

        self.error(ex_string + stack_trace_lines)

    cdef str _format_message(self, str message):
        # Add the components name to the front of the log message
        return f"{self.component_name}: {message}"


cpdef void nautilus_header(LoggerAdapter logger):
        logger.info("#---------------------------------------------------------------#")
        logger.info(f" Nautilus Trader v{__version__} by Nautech Systems Pty Ltd.")
        logger.info("#---------------------------------------------------------------#")
        logger.info("                                                                 ")
        logger.info("                            .......                              ")
        logger.info("                         .............                           ")
        logger.info("    .                  ......... .......                         ")
        logger.info("   .                  ......... .. .......                       ")
        logger.info("   .                 ......',,,,'..........                      ")
        logger.info("   ..               ......::,,''';,.........                     ")
        logger.info("   ..                ....'o:;oo;..:'..... ''                     ")
        logger.info("    ..               ......,;,,..,:'.........                    ")
        logger.info("    ..                .........';:'..... ...                     ")
        logger.info("     ..                 .......'..... .'. .'                     ")
        logger.info("      ..                   .....    .. .. ..                     ")
        logger.info("       ..                           .' ....                      ")
        logger.info("         ..                         .. .'.                       ")
        logger.info("          ....                     .....                         ")
        logger.info("             ....                ..'..                           ")
        logger.info("                 ..................                              ")
        logger.info("                                                                 ")
        logger.info("#---------------------------------------------------------------#")
        logger.info("#--- SYSTEM SPECIFICATION --------------------------------------#")
        logger.info("#---------------------------------------------------------------#")
        logger.info(f"OS: {platform.platform()}")
        logger.info(f"CPU architecture: {platform.processor()}" )
        cdef str cpu_freq_str = '' if psutil.cpu_freq() is None else f'@ {int(psutil.cpu_freq()[2])}MHz'
        logger.info(f"CPU(s): {psutil.cpu_count()} {cpu_freq_str}")
        logger.info(f"RAM-total: {round(psutil.virtual_memory()[0] / 1000000)}MB")
        logger.info("")
        logger.info("#---------------------------------------------------------------#")
        logger.info("#--- PACKAGE VERSIONS ------------------------------------------#")
        logger.info("#---------------------------------------------------------------#")
        logger.info(f"python v{python_version()}")
        logger.info(f"cython v{cython.__version__}")
        logger.info(f"numpy v{np.__version__}")
        logger.info(f"scipy v{scipy.__version__}")
        logger.info(f"pandas v{pd.__version__}")
