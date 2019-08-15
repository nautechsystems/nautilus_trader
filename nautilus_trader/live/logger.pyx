# -------------------------------------------------------------------------------------------------
# <copyright file="logger.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import queue
import multiprocessing
import threading
import redis

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logger cimport LogLevel, LogMessage


cdef class LogStore:
    """
    Provides a process and thread safe log store.
    """

    def __init__(self, TraderId trader_id, int port=6379):
        """
        Initializes a new instance of the LogStore class.

        :param trader_id: The trader identifier.
        :param port: The redis port to connect to.
        :raises ConditionFailed: If the redis_port is not in range [0, 65535].
        """
        Condition.in_range(port, 'redis_port', 0, 65535)

        self._key = f'Trader-{trader_id.value}:LogStore'
        self._redis = redis.StrictRedis(host='localhost', port=port, db=0)
        self._queue = multiprocessing.Queue()
        self._process = multiprocessing.Process(target=self._process_queue, daemon=True)
        self._process.start()

    cpdef void store(self, LogMessage message):
        """
        Store the given log message.
        
        :param message: The log message to store.
        """
        self._queue.put(message)

    cpdef void _process_queue(self):
        # Process the queue one item at a time

        cdef LogMessage message
        while True:
            message = self._queue.get()
            self._redis.rpush(f'{self._key}:{message.level_string()}', message.as_string())


cdef class LiveLogger(Logger):
    """
    Provides a thread safe logger for live concurrent operations.
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
                 str log_file_path='logs/',
                 LiveClock clock=LiveClock(),
                 LogStore store=None):
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

        self._queue = queue.Queue()
        self._store = store
        self._thread = threading.Thread(target=self._process_queue, daemon=True)
        self._thread.start()

    cpdef void log(self, LogMessage message):
        """
        Log the given message.

        :param message: The log message to log.
        """
        self._queue.put(message)

    cpdef void _process_queue(self):
        # Process the queue one item at a time
        cdef LogMessage message
        while True:
            message = self._queue.get()
            self._log(message)

            if self._store is not None and message.level >= self._log_level_store:
                self._store.store(message)
