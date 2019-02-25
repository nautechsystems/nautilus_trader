#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="logger.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

import logging
import os
import threading

from cpython.datetime cimport datetime
from threading import Thread
from queue import Queue
from logging import INFO, DEBUG

from inv_trader.core.precondition cimport Precondition
from inv_trader.model.objects cimport ValidString
from inv_trader.common.clock cimport Clock, LiveClock, TestClock


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
                 level_console: logging=INFO,
                 level_file: logging=DEBUG,
                 bint console_prints=True,
                 bint log_to_file=False,
                 str log_file_path='log/',
                 Clock clock=LiveClock()):
        """
        Initializes a new instance of the Logger class.

        :param name: The name of the logger.
        :param level_console: The minimum log level for logging messages to the console.
        :param level_file: The minimum log level for logging messages to the log file.
        :param console_prints: The boolean flag indicating whether log messages should print.
        :param log_to_file: The boolean flag indicating whether log messages should log to file
        :param log_file_path: The name of the log file (cannot be None if log_to_file is True).
        :param clock: The clock for the logger.
        :raises ValueError: If the name is not a valid string.
        :raises ValueError: If the log_file_path is not a valid string.
        """
        if name is not None:
            Precondition.valid_string(name, 'name')
        else:
            name = 'tmp'

        Precondition.valid_string(log_file_path, 'log_file_path')

        self.bypass_logging = bypass_logging
        self.clock = clock
        self._log_level_console = level_console
        self._log_level_file = level_file
        self._console_prints = console_prints
        self._log_to_file = log_to_file
        self._log_file = f'{log_file_path}{name}.log'
        self._logger = logging.getLogger(name)
        self._logger.setLevel(level_file)

        # Setup log file handling
        if log_to_file:
            if not os.path.exists(log_file_path):
                # Create directory if it does not exist
                os.makedirs(log_file_path)
            self._log_file_handler = logging.FileHandler(self._log_file)
            self._logger.addHandler(self._log_file_handler)

    cpdef void log(self, int log_level, ValidString message):
        """
        Log the given message with the given log level and timestamp.
        
        :param log_level: The log level for the log message.
        :param message: The message to log.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void _debug(self, datetime timestamp, ValidString message):
        """
        Log the given debug message with the logger.

        :param timestamp: The timestamp for the log message.
        :param message: The debug message to log.
        """
        cdef str log_message = self._format_message(timestamp, 'DBG', message.value)
        self._console_print_handler(logging.DEBUG, log_message)

        if self._log_to_file:
            try:
                self._logger.debug(log_message)
            except IOError as ex:
                self._console_print_handler(f"IOError: {ex}.", logging.CRITICAL)

    cpdef void _info(self, datetime timestamp, ValidString message):
        """
        Log the given information message with the logger.

        :param timestamp: The timestamp for the log message.
        :param message: The information message to log.
        """
        cdef str log_message = self._format_message(timestamp, 'INF', message.value)
        self._console_print_handler(logging.INFO, log_message)

        if self._log_to_file:
            try:
                self._logger.info(log_message)
            except IOError as ex:
                self._console_print_handler(f"IOError: {ex}.", logging.CRITICAL)

    cpdef void _warning(self, datetime timestamp, ValidString message):
        """
        Log the given warning message with the logger.

        :param timestamp: The timestamp for the log message.
        :param message: The warning message to log.
        """
        cdef str log_message = self._format_message(timestamp, WARNING + 'WRN' + ENDC, WARNING + message.value + ENDC)
        self._console_print_handler(logging.WARNING, log_message)

        if self._log_to_file:
            try:
                self._logger.warning(log_message)
            except IOError as ex:
                self._console_print_handler(f"IOError: {ex}.", logging.CRITICAL)

    cpdef void _error(self, datetime timestamp, ValidString message):
        """
        Log the given error message with the logger.

        :param timestamp: The timestamp for the log message.
        :param message: The error message to log.
        """
        cdef str log_message = self._format_message(timestamp, FAIL + 'ERR' + ENDC, FAIL + message.value + ENDC)
        self._console_print_handler(logging.ERROR, log_message)

        if self._log_to_file:
            try:
                self._logger.critical(log_message)
            except IOError as ex:
                self._console_print_handler(f"IOError: {ex}.", logging.CRITICAL)

    cpdef void _critical(self, datetime timestamp, ValidString message):
        """
        Log the given critical message with the logger.

        :param timestamp: The timestamp for the log message.
        :param message: The critical message to log.
        """
        cdef str log_message = self._format_message(timestamp, FAIL + 'CRT' + ENDC, FAIL + message.value + ENDC)
        self._console_print_handler(logging.CRITICAL, log_message)

        if self._log_to_file:
            try:
                self._logger.critical(log_message)
            except IOError as ex:
                self._console_print_handler(f"IOError: {ex}.", logging.CRITICAL)

    cdef str _format_message(self, datetime timestamp, str log_level, str message):
        cdef str time = timestamp.isoformat(timespec='milliseconds').partition('+')[0] + 'Z'
        return f"{BOLD}{time}{ENDC} [{threading.current_thread().ident}][{log_level}] {message}"

    cdef void _console_print_handler(self, log_level: logging, str message):
        if self._console_prints and log_level >= self._log_level_console:
            print(message)


cdef class LogMessage:
    """
    Represents a log message.
    """
    def __init__(self,
                  int log_level,
                  datetime timestamp,
                  ValidString text):
        """
        Initializes a new instance of the LogMessage class.

        :param log_level: The log_level of the message.
        :param timestamp: The timestamp of the message.
        :param text: The message text.
        """
        self.log_level = log_level
        self.timestamp = timestamp
        self.text = text


cdef class LiveLogger(Logger):
    """
    Provides a thread safe logger for live concurrent operations.
    """

    def __init__(self,
                 str name=None,
                 bint bypass_logging=False,
                 level_console: logging=INFO,
                 level_file: logging=DEBUG,
                 bint console_prints=True,
                 bint log_to_file=False,
                 str log_file_path='log/'):
        """
        Initializes a new instance of the LiveLogger class.

        :param name: The name of the logger.
        :param level_console: The minimum log level for logging messages to the console.
        :param level_file: The minimum log level for logging messages to the log file.
        :param console_prints: The boolean flag indicating whether log messages should print.
        :param log_to_file: The boolean flag indicating whether log messages should log to file
        :param log_file_path: The name of the log file (cannot be None if log_to_file is True).
        :raises ValueError: If the name is not a valid string.
        :raises ValueError: If the log_file_path is not a valid string.
        """
        super().__init__(name,
                         bypass_logging,
                         level_console,
                         level_file,
                         console_prints,
                         log_to_file,
                         log_file_path)

        self._queue = Queue()
        self._thread = Thread(target=self._process_messages, daemon=True)
        self._thread.start()

    cpdef void log(self, int log_level, ValidString message):
        """
        Log the given message with the given log level and timestamp.
        
        :param log_level: The log level for the log message.
        :param message: The message to log.
        """
        self._queue.put(LogMessage(log_level, self.clock.time_now(), message))

    cpdef void _process_messages(self):
        """
        Process the queue one item at a time.
        """
        cdef int log_level

        while True:
            log_message = self._queue.get()
            log_level = log_message.log_level

            if log_level == logging.DEBUG:
                self._debug(log_message.timestamp, log_message.text)
            elif log_level == logging.INFO:
                self._info(log_message.timestamp, log_message.text)
            elif log_level == logging.WARNING:
                self._warning(log_message.timestamp, log_message.text)
            elif log_level == logging.ERROR:
                self._error(log_message.timestamp, log_message.text)
            elif log_level == logging.CRITICAL:
                self._critical(log_message.timestamp, log_message.text)


cdef class TestLogger(Logger):
    """
    Provides a single threaded logger for testing.
    """

    def __init__(self,
                 str name=None,
                 bint bypass_logging=False,
                 level_console: logging=INFO,
                 level_file: logging=DEBUG,
                 bint console_prints=True,
                 bint log_to_file=False,
                 str log_file_path='log/',
                 Clock clock=TestClock()):
        """
        Initializes a new instance of the TestLogger class.

        :param name: The name of the logger.
        :param level_console: The minimum log level for logging messages to the console.
        :param level_file: The minimum log level for logging messages to the log file.
        :param console_prints: The boolean flag indicating whether log messages should print.
        :param log_to_file: The boolean flag indicating whether log messages should log to file
        :param log_file_path: The name of the log file (cannot be None if log_to_file is True).
        :raises ValueError: If the name is not a valid string.
        :raises ValueError: If the log_file_path is not a valid string.
        :raises ValueError: If the clock is not of type TestClock.
        """
        Precondition.type(clock, TestClock, 'clock')

        super().__init__(name,
                         bypass_logging,
                         level_console,
                         level_file,
                         console_prints,
                         log_to_file,
                         log_file_path,
                         clock)

    cpdef void log(self, int log_level, ValidString message):
        """
        Log the given message with the given log level and timestamp.
        
        :param log_level: The log level for the log message.
        :param message: The message to log.
        """
        cdef datetime time_now = self.clock.time_now()

        if log_level == logging.DEBUG:
            self._debug(time_now, message)
        elif log_level == logging.INFO:
            self._info(time_now, message)
        elif log_level == logging.WARNING:
            self._warning(time_now, message)
        elif log_level == logging.ERROR:
            self._error(time_now, message)
        elif log_level == logging.CRITICAL:
            self._critical(time_now, message)


cdef class LoggerAdapter:
    """
    Provides a logger adapter adapter for a components logger.
    """

    def __init__(self,
                 str component_name=None,
                 Logger logger=TestLogger()):
        """
        Initializes a new instance of the LoggerAdapter class.

        :param logger: The logger for the component.
        :param component_name: The name of the component.
        """
        if component_name is not None:
            Precondition.valid_string(component_name, 'component_name')
        else:
            component_name = ''

        self._logger = logger
        self.component_name = component_name
        self.bypassed = logger.bypass_logging

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

    cdef ValidString _format_message(self, str message):
        return ValidString(f"{self.component_name}: {message}")
