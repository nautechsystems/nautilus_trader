# -------------------------------------------------------------------------------------------------
# <copyright file="logger.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.__info__ import __version__
from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.functions cimport format_zulu_datetime
from nautilus_trader.common.clock cimport Clock, LiveClock, TestClock
from nautilus_trader.common.logger cimport LogLevel


cdef str _HEADER = '\033[95m'
cdef str _OK_BLUE = '\033[94m'
cdef str _OK_GREEN = '\033[92m'
cdef str _WARN = '\033[1;33m'
cdef str _FAIL = '\033[01;31m'
cdef str _ENDC = '\033[0m'
cdef str _BOLD = '\033[1m'
cdef str _UNDERLINE = '\033[4m'

RECV = '<--'
SENT = '-->'
CMD = '[CMD]'
EVT = '[EVT]'


cdef class LogMessage:
    """
    Represents a log message including timestamp and log level.
    """
    def __init__(self,
                 datetime timestamp,
                 LogLevel level,
                 str text,
                 long thread_id=0):
        """
        Initializes a new instance of the LogMessage class.

        :param timestamp: The log message timestamp.
        :param level: The log message level.
        :param text: The log message text.
        :param thread_id: The thread the log message was created on (default=0).
        """
        self.timestamp = timestamp
        self.level = level
        self.text = text
        self.thread_id = thread_id

    cdef str level_string(self):
        """
        Return the string representation of the log level.

        :return str.
        """
        return log_level_to_string(self.level)

    cdef str as_string(self):
        """
        Return the string representation of the log message.

        :return str.
        """
        return f"{format_zulu_datetime(self.timestamp)} [{self.thread_id}][{log_level_to_string(self.level)}] {self.text}"


cdef class Logger:
    """
    The base class for all Loggers.
    """

    def __init__(self,
                 str name=None,
                 bint bypass_logging=False,
                 LogLevel level_console=LogLevel.INFO,
                 LogLevel level_file=LogLevel.DEBUG,
                 LogLevel level_store=LogLevel.WARNING,
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
        :raises ConditionFailed: If the name is not a valid string.
        :raises ConditionFailed: If the log_file_path is not a valid string.
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
        self._logger.setLevel(logging.DEBUG)

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

    cpdef void log(self, LogMessage message):
        """
        Log the given log message.
        
        :param message: The log message to log.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef list get_log_store(self):
        """
        Return the log store of message strings.
        
        :return List[str].
        """
        return self._log_store

    cpdef void clear_log_store(self):
        """
        Clear the log store.
        """
        self._log_store = []

    cpdef void _log(self, LogMessage message):
        cdef str formatted_msg = self._format_output(message)
        self._in_memory_log_store(message.level, formatted_msg)
        self._print_to_console(message.level, formatted_msg)

        if self._log_to_file and message.level >= self._log_level_file:
            try:
                self._logger.debug(message.as_string())
            except IOError as ex:
                self._print_to_console(LogLevel.ERROR, f"IOError: {ex}.")

    cdef str _format_output(self, LogMessage message):
        # Return the formatted log message from the given arguments
        cdef str time = format_zulu_datetime(message.timestamp)
        cdef str thread = '' if self._log_thread is False else f'[{message.thread_id}]'
        cdef str formatted_text

        if message.level == LogLevel.WARNING:
            formatted_text = f'{_WARN}[{message.level_string()}] {message.text}{_ENDC}'
        elif message.level == LogLevel.ERROR:
            formatted_text = f'{_FAIL}[{message.level_string()}] {message.text}{_ENDC}'
        elif message.level == LogLevel.CRITICAL:
            formatted_text = f'{_FAIL}[{message.level_string()}] {message.text}{_ENDC}'
        else:
            formatted_text = f'[{message.level_string()}] {message.text}'

        return f"{_BOLD}{time}{_ENDC} {thread}{formatted_text}"

    cdef void _in_memory_log_store(self, LogLevel level, str text):
        # Store the given log message if the given log level is >= the log_level_store
        if level >= self._log_level_store:
            self._log_store.append(text)

    cdef void _print_to_console(self, LogLevel level, str text):
        # Print the given log message to the console if the given log level if
        # >= the log_level_console level.
        if self._console_prints and level >= self._log_level_console:
            print(text)


cdef class TestLogger(Logger):
    """
    Provides a single threaded logger for testing.
    """

    def __init__(self,
                 str name=None,
                 bint bypass_logging=False,
                 LogLevel level_console=LogLevel.DEBUG,
                 LogLevel level_file=LogLevel.DEBUG,
                 LogLevel level_store=LogLevel.WARNING,
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
        :raises ConditionFailed: If the name is not a valid string.
        :raises ConditionFailed: If the log_file_path is not a valid string.
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

    cpdef void log(self, LogMessage message):
        """
        Log the given log message.
        
        :param message: The log message to log.
        """
        self._log(message)


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
        
        :return logging.logger.
        """
        return self._logger

    cpdef void verbose(self, str message):
        """
        Log the given verbose message with the logger.
        
        :param message: The message to log.
        """
        self._send_to_logger(LogLevel.VERBOSE, message)

    cpdef void debug(self, str message):
        """
        Log the given debug message with the logger.

        :param message: The message to log.
        """
        self._send_to_logger(LogLevel.DEBUG, message)

    cpdef void info(self, str message):
        """
        Log the given information message with the logger.

        :param message: The message to log.
        """
        self._send_to_logger(LogLevel.INFO, message)

    cpdef void warning(self, str message):
        """
        Log the given warning message with the logger.

        :param message: The message to log.
        """
        self._send_to_logger(LogLevel.WARNING, message)

    cpdef void error(self, str message):
        """
        Log the given error message with the logger.

        :param message: The message to log.
        """
        self._send_to_logger(LogLevel.ERROR, message)

    cpdef void critical(self, str message):
        """
        Log the given critical message with the logger.

        :param message: The message to log.
        """
        self._send_to_logger(LogLevel.CRITICAL, message)

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

    cdef void _send_to_logger(self, LogLevel level, str message):
        if not self.bypassed:
            self._logger.log(LogMessage(
                self._logger.clock.time_now(),
                level,
                self._format_message(message),
                thread_id=threading.current_thread().ident))

    cdef str _format_message(self, str message):
        # Add the components name to the front of the log message
        return f"{self.component_name}: {message}"


cpdef void nautilus_header(LoggerAdapter logger):
        logger.info("#---------------------------------------------------------------#")
        logger.info(f" Nautilus Trader - Algorithmic Trading Platform")
        logger.info(f" by Nautech Systems Pty Ltd. ")
        logger.info(f" Copyright (C) 2015-2019. All rights reserved.")
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
        logger.info("#---------------------------------------------------------------#")
        logger.info("#--- VERSIONING ------------------------------------------------#")
        logger.info("#---------------------------------------------------------------#")
        logger.info(f"python {python_version()}")
        logger.info(f"cython {cython.__version__}")
        logger.info(f"numpy {np.__version__}")
        logger.info(f"scipy {scipy.__version__}")
        logger.info(f"pandas {pd.__version__}")
        logger.info(f"nautilus-trader {__version__}")
        logger.info("#---------------------------------------------------------------#")
