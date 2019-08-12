# -------------------------------------------------------------------------------------------------
# <copyright file="stores.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import redis

from multiprocessing import Queue, Process

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.events cimport Event, OrderEvent, PositionEvent
from nautilus_trader.live.logger cimport LogMessage
from nautilus_trader.serialization.serializers cimport EventSerializer, MsgPackEventSerializer


cdef class LogStore:
    """
    Provides a process and thread safe log store.
    """

    def __init__(self, TraderId trader_id, int redis_port=6379):
        """
        Initializes a new instance of the LogStore class.

        :param trader_id: The trader identifier.
        :param redis_port: The redis port to connect to.
        :raises ValueError: If the redis_port is not in range [0, 65535].
        """
        Condition.in_range(redis_port, 'redis_port', 0, 65535)

        self._store_key = f'Nautilus:Traders:{trader_id.value}:LogStore'
        self._redis = redis.StrictRedis(host='localhost', port=redis_port, db=0)
        self._queue = Queue()
        self._process = Process(target=self._process_queue, daemon=True)
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
            self._redis.rpush(self._store_key, message.as_string())


cdef class EventStore:
    """
    Provides a process and thread safe event store.
    """

    def __init__(self,
                 TraderId trader_id,
                 int redis_port=6379,
                 EventSerializer serializer=MsgPackEventSerializer()):
        """
        Initializes a new instance of the EventStore class.

        :param trader_id: The trader identifier.
        :param redis_port: The redis port to connect to.
        :param serializer: The event serializer.
        :raises ValueError: If the redis_port is not in range [0, 65535].
        """
        Condition.in_range(redis_port, 'redis_port', 0, 65535)

        self._store_key = f'Nautilus:Traders:{trader_id.value}'
        self._serializer = serializer
        self._redis = redis.StrictRedis(host='localhost', port=redis_port, db=0)
        self._queue = Queue()
        self._process = Process(target=self._process_queue, daemon=True)
        self._process.start()

    cpdef void store(self, Event message):
        """
        Store the given event message.
        
        :param message: The event message to store.
        """
        self._queue.put(message)

    cpdef void _process_queue(self):
        # Process the queue one item at a time
        cdef Event event
        while True:
            event = self._queue.get()

            if isinstance(event, OrderEvent):
                self._store_order_event(event)

    cdef void _store_order_event(self, OrderEvent event):
        self._redis.rpush(self._store_key + ':' + event.order_id, self._serializer.serialize(event))
