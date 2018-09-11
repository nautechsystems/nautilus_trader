#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="factories.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import logging
import threading

from datetime import datetime
from logging import INFO, DEBUG

from inv_trader.core.precondition import Precondition


class Logger:
    """
    Provides a logger for the trader client which wraps the Python logging module.
    """

    def __init__(self,
                 log_name=None,
                 log_level_console: logging=INFO,
                 log_level_file: logging=DEBUG,
                 console_prints: bool=True,
                 log_to_file: bool=False,
                 log_file_path: str='var/tmp/'):
        """
        Initializes a new instance of the Logger class.

        :param: log_name: The name of the logger.
        :param: log_level_console: The minimum log level for logging messages to the console.
        :param: log_level_file: The minimum log level for logging messages to the log file.
        :param: console_prints: The boolean flag indicating whether log messages should print.
        :param: log_to_file: The boolean flag indicating whether log messages should log to file
        :param: log_file_name: The name of the log file (cannot be None if log_to_file is True).
        """
        if log_name is not None:
            Precondition.valid_string(log_name, 'log_name')
        else:
            log_name = 'tmp'

        Precondition.valid_string(log_file_path, 'log_file_path')

        self._log_level_console = log_level_console
        self._log_level_file = log_level_file
        self._console_prints = console_prints
        self._log_to_file = log_to_file
        self._log_file = f'{log_file_path}{log_name}.log'
        self._logger = logging.getLogger(log_name)
        self._logger.setLevel(log_level_file)

        # Setup log file handling.
        if log_to_file:
            self._log_file_handler = logging.FileHandler(self._log_file)
            self._logger.addHandler(self._log_file_handler)

    def debug(self, message: str):
        """
        Log the given debug message with the logger.

        :param message: The debug message to log.
        """
        Precondition.valid_string(message, 'message')

        log_message = self._format_message('DBG', message)
        self._console_print_handler(log_message, logging.DEBUG)

        if self._log_to_file:
            self._logger.debug(log_message)

    def info(self, message: str):
        """
        Log the given information message with the logger.

        :param message: The information message to log.
        """
        Precondition.valid_string(message, 'message')

        log_message = self._format_message('INF', message)
        self._console_print_handler(log_message, logging.INFO)

        if self._log_to_file:
            self._logger.info(log_message)

    def warning(self, message: str):
        """
        Log the given warning message with the logger.

        :param message: The warning message to log.
        """
        Precondition.valid_string(message, 'message')

        log_message = self._format_message('WRN', message)
        self._console_print_handler(log_message, logging.WARNING)

        if self._log_to_file:
            self._logger.warning(log_message)

    def critical(self, message: str):
        """
        Log the given critical message with the logger.

        :param message: The critical message to log.
        """
        Precondition.valid_string(message, 'message')

        log_message = self._format_message('FTL', message)
        self._console_print_handler(log_message, logging.CRITICAL)

        if self._log_to_file:
            self._logger.critical(log_message)

    @staticmethod
    def _format_message(
            log_level: str,
            message: str):

        time = datetime.utcnow().isoformat(timespec='milliseconds') + 'Z'
        return (f'{time} [{threading.current_thread().ident}][{log_level}] '
                f'{message}')

    def _console_print_handler(
            self,
            message: str,
            log_level: logging):

        if self._console_prints and self._log_level_console <= log_level:
            print(message)


class LoggingAdapter:
    """
    Provides an adapter for the trading clients logger.
    """

    def __init__(
            self,
            component_name: str = None,
            logger: Logger=Logger()):
        """
        Initializes a new instance of the LoggingAdapter class.

        :param: logger: The logger for the component.
        :param: component_name: The name of the component.
        """
        if component_name is not None:
            Precondition.valid_string(component_name, 'component_name')
            component_name = component_name + ':'
        else:
            component_name = ''

        self._logger = logger
        self._component_name = component_name

    def debug(self, message: str):
        """
        Log the given debug message with the logger.

        :param message: The debug message to log.
        """
        Precondition.valid_string(message, 'message')

        self._logger.debug(self._format_message(message))

    def info(self, message: str):
        """
        Log the given information message with the logger.

        :param message: The information message to log.
        """
        Precondition.valid_string(message, 'message')

        self._logger.info(self._format_message(message))

    def warning(self, message: str):
        """
        Log the given warning message with the logger.

        :param message: The warning message to log.
        """
        Precondition.valid_string(message, 'message')

        self._logger.warning(self._format_message(message))

    def critical(self, message: str):
        """
        Log the given critical message with the logger.

        :param message: The critical message to log.
        """
        Precondition.valid_string(message, 'message')

        self._logger.critical(self._format_message(message))

    def _format_message(self, message: str):
        return f"{self._component_name} {message}"
