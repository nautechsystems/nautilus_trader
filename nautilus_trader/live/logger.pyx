# -------------------------------------------------------------------------------------------------
# <copyright file="logger.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import logging

from cpython.datetime cimport datetime
from threading import Thread
from queue import Queue

from nautilus_trader.core.functions cimport format_zulu_datetime
from nautilus_trader.common.clock cimport LiveClock


cdef class LogMessage:
    """
    Represents a log message.
    """
    def __init__(self,
                 datetime timestamp,
                 int level,
                 str text):
        """
        Initializes a new instance of the LogMessage class.

        :param timestamp: The log message timestamp.
        :param level: The log message level.
        :param text: The log message text.
        """
        self.timestamp = timestamp
        self.level = level
        self.text = text

    cdef str as_string(self):
        """
        Return the string representation of the log message.
        
        :return: str.
        """
        return f"{format_zulu_datetime(self.timestamp)} [{self.level}] {self.text}"


cdef class LiveLogger(Logger):
    """
    Provides a thread safe logger for live concurrent operations.
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
                 str log_file_path='logs/',
                 LiveClock clock=LiveClock()):
        """
        Initializes a new instance of the LiveLogger class.

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

        self._queue = Queue()
        self._thread = Thread(target=self._process_messages, daemon=True)
        self._thread.start()

    cpdef void log(self, int level, str message):
        """
        Log the given message with the given log level.
        
        :param level: The log level for the log message.
        :param message: The message to log.
        """
        self._queue.put(LogMessage(self.clock.time_now(), level, message))

    cpdef void _process_messages(self):
        cdef LogMessage message
        while True:
            # Process the queue one item at a time
            message = self._queue.get()

            if message.level == logging.DEBUG:
                self._debug(message.timestamp, message.text)
            elif message.level == logging.INFO:
                self._info(message.timestamp, message.text)
            elif message.level == logging.WARNING:
                self._warning(message.timestamp, message.text)
            elif message.level == logging.ERROR:
                self._error(message.timestamp, message.text)
            elif message.level == logging.CRITICAL:
                self._critical(message.timestamp, message.text)
            else:
                raise RuntimeError(f"Log level {message.level} not recognized.")

            self._queue.task_done()
