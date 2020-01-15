# -------------------------------------------------------------------------------------------------
# <copyright file="logger.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import queue
import multiprocessing
import redis
import threading

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logger cimport LogLevel, LogMessage
from nautilus_trader.serialization.base cimport LogSerializer
from nautilus_trader.serialization.serializers cimport MsgPackLogSerializer


cdef class LogStore:
    """
    Provides a process and thread safe log store.
    """

    def __init__(self,
                 TraderId trader_id,
                 str host='localhost',
                 int port=6379,
                 LogSerializer serializer=MsgPackLogSerializer()):
        """
        Initializes a new instance of the LogStore class.

        :param trader_id: The trader_id.
        :param host: The redis host to connect to.
        :param port: The redis port to connect to.
        :raises ConditionFailed: If the redis_host is not a valid string.
        :raises ConditionFailed: If the redis_port is not in range [0, 65535].
        """
        Condition.valid_string(host, 'host')
        Condition.valid_port(port, 'port')

        self._key = f'Trader-{trader_id.value}:LogStore'
        self._redis = redis.Redis(host=host, port=port, db=0)
        self._message_bus = multiprocessing.Queue()
        self._serializer = serializer
        self._process = multiprocessing.Process(target=self._consume_messages, daemon=True)
        self._process.start()

    cpdef void store(self, LogMessage message):
        """
        Store the given log message.
        
        :param message: The log message to store.
        """
        self._message_bus.put(message)

    cpdef void _consume_messages(self) except *:
        cdef LogMessage message
        while True:
            message = self._message_bus.get()
            self._redis.rpush(f'{self._key}:{message.level_string()}', self._serializer.serialize(message))


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
        :param console_prints: If log messages should print to the console.
        :param log_thread: If log messages should include the thread.
        :param log_to_file: If log messages should write to the log file.
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

        self._message_bus = queue.Queue()
        self._store = store
        self._thread = threading.Thread(target=self._consume_messages, daemon=True)
        self._thread.start()

    cpdef void log(self, LogMessage message) except *:
        """
        Log the given message.

        :param message: The log message to log.
        """
        self._message_bus.put(message)

    cpdef void _consume_messages(self) except *:
        cdef LogMessage message
        while True:
            message = self._message_bus.get()
            self._log(message)

            if self._store is not None and message.level >= self._log_level_store:
                self._store.store(message)
