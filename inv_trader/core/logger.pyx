#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="logger.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

import logging
import os
import threading

from datetime import datetime
from logging import INFO, DEBUG

from inv_trader.core.precondition cimport Precondition


cdef class Logger:
    """
    Provides a logger for the trader client which wraps the Python logging module.
    """
    cdef object _log_level_console
    cdef object _log_level_file
    cdef bint _console_prints
    cdef bint _log_to_file
    cdef str _log_file
    cdef object _logger

    def __init__(self,
                 name=None,
                 level_console: logging=INFO,
                 level_file: logging=DEBUG,
                 bint console_prints=True,
                 bint log_to_file=False,
                 str log_file_path='log/'):
        """
        Initializes a new instance of the Logger class.

        :param name: The name of the logger.
        :param level_console: The minimum log level for logging messages to the console.
        :param level_file: The minimum log level for logging messages to the log file.
        :param console_prints: The boolean flag indicating whether log messages should print.
        :param log_to_file: The boolean flag indicating whether log messages should log to file
        :param log_file_path: The name of the log file (cannot be None if log_to_file is True).
        :raises ValueError: If the name is not a valid string.
        :raises ValueError: If the log_file_path is not a valid string.
        """
        if name is not None:
            Precondition.valid_string(name, 'name')
        else:
            name = 'tmp'

        Precondition.valid_string(log_file_path, 'log_file_path')

        self._log_level_console = level_console
        self._log_level_file = level_file
        self._console_prints = console_prints
        self._log_to_file = log_to_file
        self._log_file = f'{log_file_path}{name}.log'
        self._logger = logging.getLogger(name)
        self._logger.setLevel(level_file)

        # Setup log file handling.
        if log_to_file:
            # Create directory if it does not exist.
            if not os.path.exists(log_file_path):
                os.makedirs(log_file_path)
            self._log_file_handler = logging.FileHandler(self._log_file)
            self._logger.addHandler(self._log_file_handler)

    cpdef void debug(self, str message):
        """
        Log the given debug message with the logger.

        :param message: The debug message to log.
        """
        Precondition.valid_string(message, 'message')

        log_message = self._format_message('DBG', message)
        self._console_print_handler(log_message, logging.DEBUG)

        if self._log_to_file:
            try:
                self._logger.debug(log_message)
            except IOError as ex:
                self._console_print_handler(f"IOError: {ex}.", logging.CRITICAL)

    cpdef void info(self, str message):
        """
        Log the given information message with the logger.

        :param message: The information message to log.
        """
        Precondition.valid_string(message, 'message')

        log_message = self._format_message('INF', message)
        self._console_print_handler(log_message, logging.INFO)

        if self._log_to_file:
            try:
                self._logger.info(log_message)
            except IOError as ex:
                self._console_print_handler(f"IOError: {ex}.", logging.CRITICAL)

    cpdef void warning(self, str message):
        """
        Log the given warning message with the logger.

        :param message: The warning message to log.
        """
        Precondition.valid_string(message, 'message')

        log_message = self._format_message('WRN', message)
        self._console_print_handler(log_message, logging.WARNING)

        if self._log_to_file:
            try:
                self._logger.warning(log_message)
            except IOError as ex:
                self._console_print_handler(f"IOError: {ex}.", logging.CRITICAL)

    cpdef void critical(self, str message):
        """
        Log the given critical message with the logger.

        :param message: The critical message to log.
        """
        Precondition.valid_string(message, 'message')

        log_message = self._format_message('FTL', message)
        self._console_print_handler(log_message, logging.CRITICAL)

        if self._log_to_file:
            try:
                self._logger.critical(log_message)
            except IOError as ex:
                self._console_print_handler(f"IOError: {ex}.", logging.CRITICAL)

    cdef str _format_message(
            self,
            str log_level,
            str message):

        time = datetime.utcnow().isoformat(timespec='milliseconds') + 'Z'
        return (f'{time} [{threading.current_thread().ident}][{log_level}] '
                f'{message}')

    cdef void _console_print_handler(
            self,
            str message,
            log_level: logging):

        if self._console_prints and self._log_level_console <= log_level:
            print(message)


cdef class LoggerAdapter:
    """
    Provides a logger adapter adapter for a components logger.
    """
    cdef str _component_name
    cdef object _logger

    def __init__(self,
                 str component_name=None,
                 logger: Logger=Logger()):
        """
        Initializes a new instance of the LoggerAdapter class.

        :param logger: The logger for the component.
        :param component_name: The name of the component.
        """
        if component_name is not None:
            Precondition.valid_string(component_name, 'component_name')
        else:
            component_name = ''

        self._component_name = component_name
        self._logger = logger


    cpdef void debug(self, str message):
        """
        Log the given debug message with the logger.

        :param message: The debug message to log.
        """
        Precondition.valid_string(message, 'message')

        self._logger.debug(self._format_message(message))

    cpdef void info(self, str message):
        """
        Log the given information message with the logger.

        :param message: The information message to log.
        """
        Precondition.valid_string(message, 'message')

        self._logger.info(self._format_message(message))

    cpdef void warning(self, str message):
        """
        Log the given warning message with the logger.

        :param message: The warning message to log.
        """
        Precondition.valid_string(message, 'message')

        self._logger.warning(self._format_message(message))

    cpdef void critical(self, str message):
        """
        Log the given critical message with the logger.

        :param message: The critical message to log.
        """
        Precondition.valid_string(message, 'message')

        self._logger.critical(self._format_message(message))

    cdef str _format_message(self, str message):
        return f"{self._component_name}: {message}"
