# -------------------------------------------------------------------------------------------------
# <copyright file="network.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import zmq

from typing import Callable
from threading import Thread
from zmq import Context

from nautilus_trader.core.precondition cimport Precondition
from nautilus_trader.common.logger cimport Logger, LoggerAdapter

cdef str UTF8 = 'utf-8'


cdef class MQWorker:
    """
    The abstract base class for all MQ workers.
    """

    def __init__(
            self,
            str worker_name,
            str service_name,
            str service_address,
            int service_port,
            context: Context,
            int socket_type,
            Logger logger=None):
        """
        Initializes a new instance of the MQWorker class.

        :param worker_name: The name of the worker.
        :param service_address: The service name.
        :param service_address: The service host address.
        :param service_port: The service port.
        :param context: The ZeroMQ context.
        :param socket_type: The ZeroMQ socket type.
        :param logger: The logger for the component.
        :raises ValueError: If the worker_name is not a valid string.
        :raises ValueError: If the service_name is not a valid string.
        :raises ValueError: If the service_address is not a valid string.
        :raises ValueError: If the service_port is not in range [0, 65535].
        """
        Precondition.valid_string(worker_name, 'worker_name')
        Precondition.valid_string(service_name, 'service_name')
        Precondition.valid_string(service_address, 'service_address')
        Precondition.in_range(service_port, 'service_port', 0, 65535)

        super().__init__()
        self._thread = Thread(target=self._open_connection, daemon=True)
        self.name = worker_name
        self._service_name = service_name
        self._service_address = f'tcp://{service_address}:{service_port}'
        self._context = context
        self._socket = self._context.socket(socket_type)
        self._socket.setsockopt(zmq.LINGER, 0)
        self._log = LoggerAdapter(worker_name, logger)
        self._cycles = 0

    cpdef void start(self):
        """
        Starts the worker and opens a connection.
        
        Overrides the threads run method (.start() should be called to run this
        in a separate thread).
        """
        self._thread.start()

    cpdef void stop(self):
        """
        Close the connection and stop the worker.
        """
        self._close_connection()
        self._log.debug(f"Stopped.")

    cpdef void _open_connection(self):
        # Open a connection to the service
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void _close_connection(self):
        # Close the connection with the service
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")


cdef class RequestWorker(MQWorker):
    """
    Provides an asynchronous worker thread for ZMQ request messaging.
    """

    def __init__(
            self,
            str worker_name,
            str service_name,
            str service_address,
            int service_port,
            context: Context,
            Logger logger=None):
        """
        Initializes a new instance of the RequestWorker class.

        :param worker_name: The name of the worker.
        :param service_name: The service name.
        :param service_address: The service host address.
        :param service_port: The service port.
        :param context: The ZeroMQ context.
        :param logger: The logger for the component.
        :raises ValueError: If the worker_name is not a valid string.
        :raises ValueError: If the service_name is not a valid string.
        :raises ValueError: If the service_address is not a valid string.
        :raises ValueError: If the service_port is not in range [0, 65535].
        """
        Precondition.valid_string(worker_name, 'worker_name')
        Precondition.valid_string(service_name, 'service_name')
        Precondition.valid_string(service_address, 'service_address')
        Precondition.in_range(service_port, 'service_port', 0, 65535)

        super().__init__(
            worker_name,
            service_name,
            service_address,
            service_port,
            context,
            zmq.REQ,
            logger)

    cpdef void send(self, bytes request, handler: Callable, callback: Callable):
        """
        Send the given message to the service socket.

        :param request: The request message bytes to send.
        :param handler: The handler for the response message.
        :param callback: The callback for the response message.
        """
        self._socket.send(request)
        self._cycles += 1
        self._log.debug(f"Sending[{self._cycles}] request {request}")

        cdef bytes response = self._socket.recv()
        handler(response, callback)
        self._log.debug(f"Received[{self._cycles}] response {response}.")

    cpdef void _open_connection(self):
        # Open a connection to the service
        self._socket.connect(self._service_address)
        self._log.info(f"Connected to {self._service_name} at {self._service_address}")

    cpdef void _close_connection(self):
        # Close the connection with the service
        self._socket.disconnect(self._service_address)
        self._log.info(f"Disconnected from {self._service_name} at {self._service_address}")


cdef class SubscriberWorker(MQWorker):
    """
    Provides an asynchronous worker thread for ZMQ subscriber messaging.
    """

    def __init__(
            self,
            str worker_name,
            str service_name,
            str service_address,
            int service_port,
            context: Context,
            handler: Callable,
            Logger logger=None):
        """
        Initializes a new instance of the SubscriberWorker class.

        :param worker_name: The name of the worker.
        :param service_name: The service name.
        :param service_address: The service host address.
        :param service_port: The service port.
        :param context: The ZeroMQ context.
        :param handler: The message handler.
        :param logger: The logger for the component.
        :raises ValueError: If the name is not a valid string.
        :raises ValueError: If the host is not a valid string.
        :raises ValueError: If the port is not in range [0, 65535].
        :raises ValueError: If the topic is not a valid string.
        """
        Precondition.valid_string(worker_name, 'worker_name')
        Precondition.valid_string(service_address, 'service_address')
        Precondition.in_range(service_port, 'port', 0, 65535)

        super().__init__(
            worker_name,
            service_address,
            service_port,
            context,
            zmq.SUB,
            logger)

        self._handler = handler

    cpdef void subscribe(self, str topic):
        """
        Subscribe the worker to the given topic.
        :param topic: The topic to subscribe to.
        """
        self._socket.setsockopt(zmq.SUBSCRIBE, self._topic.encode(UTF8))
        self._log.info(f"Subscribed to topic {self._topic}.")

    cpdef void unsubscribe(self, str topic):
        """
        Unsubscribe the worker from the given topic.
        :param topic: The topic to unsubscribe from.
        """
        self._socket.setsockopt(zmq.UNSUBSCRIBE, self._topic.encode(UTF8))
        self._log.info(f"Unsubscribed from topic {self._topic}.")

    cpdef void _open_connection(self):
        # Open a connection to the service
        self._socket.connect(self._service_address)
        self._log.info(f"Connected to {self._service_name} at {self._service_address}")
        self._consume_messages()

    cpdef void _consume_messages(self):
        # Start the consumption loop to receive published messages
        self._log.info("Ready to consume messages...")

        cdef bytes message
        cdef bytes topic
        cdef bytes body
        cdef str topic_str

        while True:
            message = self._socket.recv()

            # Split on first occurrence of empty byte delimiter
            topic, body = message.split(b' ', 1)
            topic_str = topic.decode(UTF8)

            self._handler(topic_str, body)
            self._cycles += 1
            self._log.debug(f"Received[{self._cycles}] message for topic {topic_str}: {body}")

    cpdef void _close_connection(self):
        # Close the connection with the service
        self._socket.disconnect(self._service_address)
        self._log.info(f"Disconnected from {self._service_name} at {self._service_address}")
