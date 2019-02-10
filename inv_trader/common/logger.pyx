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

from threading import Thread, Lock
from queue import Queue
from logging import INFO, DEBUG

from inv_trader.core.precondition cimport Precondition
from inv_trader.model.objects cimport ValidString
from inv_trader.common.clock cimport Clock, LiveClock


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
    Provides a logger for the trader client which wraps the Python logging module.
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
        self._clock = clock
        self._log_level_console = level_console
        self._log_level_file = level_file
        self._console_prints = console_prints
        self._log_to_file = log_to_file
        self._log_file = f'{log_file_path}{name}.log'
        self._logger = logging.getLogger(name)
        self._logger.setLevel(level_file)
        self._queue = Queue()
        self._thread = Thread(target=self._process_messages, daemon=True)

        # Setup log file handling.
        if log_to_file:
            # Create directory if it does not exist.
            if not os.path.exists(log_file_path):
                os.makedirs(log_file_path)
            self._log_file_handler = logging.FileHandler(self._log_file)
            self._logger.addHandler(self._log_file_handler)

        self._thread.start()

    cpdef void log(self, tuple message):
        """
        TBA
        :param message: 
        :return: 
        """
        self._queue.put(message)

    cpdef void _debug(self, ValidString message):
        """
        Log the given debug message with the logger.

        :param message: The debug message to log.
        """
        cdef str log_message = self._format_message('DBG', message.value)
        self._console_print_handler(logging.DEBUG, log_message)

        if self._log_to_file:
            try:
                self._logger.debug(log_message)
            except IOError as ex:
                self._console_print_handler(f"IOError: {ex}.", logging.CRITICAL)

    cpdef void _info(self, ValidString message):
        """
        Log the given information message with the logger.

        :param message: The information message to log.
        """
        cdef str log_message = self._format_message('INF', message.value)
        self._console_print_handler(logging.INFO, log_message)

        if self._log_to_file:
            try:
                self._logger.info(log_message)
            except IOError as ex:
                self._console_print_handler(f"IOError: {ex}.", logging.CRITICAL)

    cpdef void _warning(self, ValidString message):
        """
        Log the given warning message with the logger.

        :param message: The warning message to log.
        """
        cdef str log_message = self._format_message(WARNING + 'WRN' + ENDC, WARNING + message.value + ENDC)
        self._console_print_handler(logging.WARNING, log_message)

        if self._log_to_file:
            try:
                self._logger.warning(log_message)
            except IOError as ex:
                self._console_print_handler(f"IOError: {ex}.", logging.CRITICAL)

    cpdef void _error(self, ValidString message):
        """
        Log the given error message with the logger.

        :param message: The error message to log.
        """
        cdef str log_message = self._format_message(FAIL + 'ERR' + ENDC, FAIL + message.value + ENDC)
        self._console_print_handler(logging.ERROR, log_message)

        if self._log_to_file:
            try:
                self._logger.critical(log_message)
            except IOError as ex:
                self._console_print_handler(f"IOError: {ex}.", logging.CRITICAL)

    cpdef void _critical(self, ValidString message):
        """
        Log the given critical message with the logger.

        :param message: The critical message to log.
        """
        cdef str log_message = self._format_message(FAIL + 'CRT' + ENDC, FAIL + message.value + ENDC)
        self._console_print_handler(logging.CRITICAL, log_message)

        if self._log_to_file:
            try:
                self._logger.critical(log_message)
            except IOError as ex:
                self._console_print_handler(f"IOError: {ex}.", logging.CRITICAL)

    cpdef void _process_messages(self):
        """
        Process the queue one item at a time.
        """
        while True:
            item = self._queue.get()

            if item[0] == logging.DEBUG:
                self._debug(item[1])
            elif item[0] == logging.INFO:
                self._info(item[1])
            elif item[0] == logging.WARNING:
                self._warning(item[1])
            elif item[0] == logging.ERROR:
                self._error(item[1])
            elif item[0] == logging.CRITICAL:
                self._critical(item[1])

    cdef str _format_message(self, str log_level, str message):
        #cdef str time = self._clock.time_now().isoformat(timespec='milliseconds') + 'Z'
        cdef str time = self._clock.time_now().isoformat() + 'Z'
        return f"{BOLD}{time}{ENDC} [{threading.current_thread().ident}][{log_level}] {message}"

    cdef void _console_print_handler(self, log_level: logging, str message):
        if self._console_prints and log_level >= self._log_level_console:
            with Lock():
                print(message)


cdef class LoggerAdapter:
    """
    Provides a logger adapter adapter for a components logger.
    """

    def __init__(self,
                 str component_name=None,
                 Logger logger=Logger()):
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
            self._logger.log((logging.DEBUG, self._format_message(message)))

    cpdef void info(self, str message):
        """
        Log the given information message with the logger.

        :param message: The information message to log.
        """
        if not self.bypassed:
            self._logger.log((logging.INFO, self._format_message(message)))

    cpdef void warning(self, str message):
        """
        Log the given warning message with the logger.

        :param message: The warning message to log.
        """
        if not self.bypassed:
            self._logger.log((logging.WARNING, self._format_message(message)))

    cpdef void error(self, str message):
        """
        Log the given error message with the logger.

        :param message: The error message to log.
        """
        if not self.bypassed:
            self._logger.log((logging.ERROR, self._format_message(message)))

    cpdef void critical(self, str message):
        """
        Log the given critical message with the logger.

        :param message: The critical message to log.
        """
        if not self.bypassed:
            self._logger.log((logging.CRITICAL, self._format_message(message)))

    cdef ValidString _format_message(self, str message):
        return ValidString(f"{self.component_name}: {message}")
