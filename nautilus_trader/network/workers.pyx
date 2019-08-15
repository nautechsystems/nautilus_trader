# -------------------------------------------------------------------------------------------------
# <copyright file="workers.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import zmq

from typing import Callable
from threading import Thread
from zmq import Context

from nautilus_trader.core.correctness cimport Condition
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
            zmq_context: Context,
            int zmq_socket_type,
            Logger logger=None):
        """
        Initializes a new instance of the MQWorker class.

        :param worker_name: The name of the worker.
        :param service_address: The service name.
        :param service_address: The service host address.
        :param service_port: The service port.
        :param zmq_context: The ZeroMQ context.
        :param zmq_socket_type: The ZeroMQ socket type.
        :param logger: The logger for the component.
        :raises ConditionFailed: If the worker_name is not a valid string.
        :raises ConditionFailed: If the service_name is not a valid string.
        :raises ConditionFailed: If the service_address is not a valid string.
        :raises ConditionFailed: If the service_port is not in range [0, 65535].
        """
        Condition.valid_string(worker_name, 'worker_name')
        Condition.valid_string(service_name, 'service_name')
        Condition.valid_string(service_address, 'service_address')
        Condition.in_range(service_port, 'service_port', 0, 65535)
        Condition.type(zmq_context, Context, 'zmq_context')

        super().__init__()
        self.name = worker_name
        self._service_name = service_name
        self._service_address = f'tcp://{service_address}:{service_port}'
        self._zmq_context = zmq_context
        self._zmq_socket = self._zmq_context.socket(zmq_socket_type)
        self._zmq_socket.setsockopt(zmq.LINGER, 0)
        self._log = LoggerAdapter(worker_name, logger)
        self._cycles = 0

    cpdef void connect(self):
        """
        Connect to the service.
        """
        self._zmq_socket.connect(self._service_address)
        self._log.info(f"Connected to {self._service_name} at {self._service_address}")

    cpdef void disconnect(self):
        """
        Disconnect from the service.
        :return: 
        """
        self._zmq_socket.disconnect(self._service_address)
        self._log.info(f"Disconnected from {self._service_name} at {self._service_address}")

    cpdef void dispose(self):
        """
        Dispose of the MQWorker which close the socket (call disconnect first).
        """
        self._zmq_socket.close()
        self._log.debug(f"Disposed.")


cdef class RequestWorker(MQWorker):
    """
    Provides a worker for ZMQ requester messaging.
    """

    def __init__(
            self,
            str worker_name,
            str service_name,
            str service_address,
            int service_port,
            zmq_context: Context,
            Logger logger=None):
        """
        Initializes a new instance of the RequestWorker class.

        :param worker_name: The name of the worker.
        :param service_name: The service name.
        :param service_address: The service host address.
        :param service_port: The service port.
        :param zmq_context: The ZeroMQ context.
        :param logger: The logger for the component.
        :raises ConditionFailed: If the worker_name is not a valid string.
        :raises ConditionFailed: If the service_name is not a valid string.
        :raises ConditionFailed: If the service_address is not a valid string.
        :raises ConditionFailed: If the service_port is not in range [0, 65535].
        """
        Condition.valid_string(worker_name, 'worker_name')
        Condition.valid_string(service_name, 'service_name')
        Condition.valid_string(service_address, 'service_address')
        Condition.in_range(service_port, 'service_port', 0, 65535)
        Condition.type(zmq_context, Context, 'zmq_context')

        super().__init__(
            worker_name,
            service_name,
            service_address,
            service_port,
            zmq_context,
            zmq.REQ,
            logger)

    cpdef bytes send(self, bytes request):
        """
        Send the given message to the service socket.

        :param request: The request message bytes to send.
        """
        Condition.not_empty(request, 'request')

        self._cycles += 1
        self._zmq_socket.send(request)
        self._log.verbose(f"[{self._cycles}]--> Request of {len(request)} bytes.")

        cdef bytes response = self._zmq_socket.recv()
        self._log.verbose(f"[{self._cycles}]<-- Response of {len(response)} bytes.")

        return response


cdef class SubscriberWorker(MQWorker):
    """
    Provides an asynchronous worker for ZMQ subscriber messaging.
    """

    def __init__(
            self,
            str worker_name,
            str service_name,
            str service_address,
            int service_port,
            zmq_context: Context,
            handler: Callable,
            Logger logger=None):
        """
        Initializes a new instance of the SubscriberWorker class.

        :param worker_name: The name of the worker.
        :param service_name: The service name.
        :param service_address: The service host address.
        :param service_port: The service port.
        :param zmq_context: The ZeroMQ context.
        :param handler: The message handler.
        :param logger: The logger for the component.
        :raises ConditionFailed: If the worker_name is not a valid string.
        :raises ConditionFailed: If the service_name is not a valid string.
        :raises ConditionFailed: If the port is not in range [0, 65535].
        :raises ConditionFailed: If the topic is not a valid string.
        """
        Condition.valid_string(worker_name, 'worker_name')
        Condition.valid_string(service_name, 'service_name')
        Condition.valid_string(service_address, 'service_address')
        Condition.in_range(service_port, 'port', 0, 65535)
        Condition.type(handler, Callable, 'handler')

        super().__init__(
            worker_name,
            service_name,
            service_address,
            service_port,
            zmq_context,
            zmq.SUB,
            logger)
        self._thread = Thread(target=self._consume_messages, daemon=True)
        self._handler = handler

        self._thread.start()

    cpdef void subscribe(self, str topic):
        """
        Subscribe the worker to the given topic.
        
        :param topic: The topic to subscribe to.
        """
        self._zmq_socket.setsockopt(zmq.SUBSCRIBE, topic.encode(UTF8))
        self._log.debug(f"Subscribed to topic {topic}.")

    cpdef void unsubscribe(self, str topic):
        """
        Unsubscribe the worker from the given topic.
        
        :param topic: The topic to unsubscribe from.
        """
        self._zmq_socket.setsockopt(zmq.UNSUBSCRIBE, topic.encode(UTF8))
        self._log.debug(f"Unsubscribed from topic {topic}.")

    cpdef void _consume_messages(self):
        self._log.debug("Running...")

        cdef str topic
        cdef bytes body
        while True:
            self._cycles += 1
            topic = self._zmq_socket.recv().decode(UTF8)
            body = self._zmq_socket.recv()

            self._log.verbose(f"[{self._cycles}]<-- topic={topic}, message={body}")
            self._handler(topic, body)
