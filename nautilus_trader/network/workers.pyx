# -------------------------------------------------------------------------------------------------
# <copyright file="workers.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import os
import threading
import zmq
import zmq.auth

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.common.logger cimport Logger, LoggerAdapter
from nautilus_trader.network.encryption cimport EncryptionConfig

cdef str _UTF8 = 'utf-8'


cdef class MQWorker:
    """
    The base class for all message queue workers.
    """

    def __init__(
            self,
            str worker_name,
            str service_name,
            str host,
            int port,
            zmq_context not None: zmq.Context,
            int zmq_socket_type,
            EncryptionConfig encryption not None,
            Logger logger not None):
        """
        Initializes a new instance of the MQWorker class.

        :param worker_name: The name of the worker.
        :param host: The service name.
        :param host: The service host address.
        :param port: The service port.
        :param zmq_context: The ZeroMQ context.
        :param zmq_socket_type: The ZeroMQ socket type.
        :param encryption: The encryption configuration.
        :param logger: The logger for the component.
        :raises ValueError: If the worker_name is not a valid string.
        :raises ValueError: If the service_name is not a valid string.
        :raises ValueError: If the host is not a valid string.
        :raises ValueError: If the port is not in range [0, 65535].
        """
        Condition.valid_string(worker_name, 'worker_name')
        Condition.valid_string(service_name, 'service_name')
        Condition.valid_string(host, 'host')
        Condition.valid_port(port, 'port')
        Condition.type(zmq_context, zmq.Context, 'zmq_context')
        super().__init__()

        self.name = worker_name
        self._service_name = service_name
        self._service_address = f'tcp://{host}:{port}'
        self._zmq_context = zmq_context
        self._zmq_socket = self._zmq_context.socket(zmq_socket_type)
        self._zmq_socket.setsockopt(zmq.LINGER, 1)
        self._log = LoggerAdapter(worker_name, logger)
        self._cycles = 0

        if encryption.use_encryption:
            key_file_client = os.path.join(encryption.keys_dir, "client.key")
            key_file_server = os.path.join(encryption.keys_dir, "server.key")
            client_public, client_secret = zmq.auth.load_certificate(key_file_client)
            server_public, server_secret = zmq.auth.load_certificate(key_file_server)
            self._zmq_socket.curve_secretkey = client_secret
            self._zmq_socket.curve_publickey = client_public
            self._zmq_socket.curve_serverkey = server_public

    cpdef void connect(self) except *:
        """
        Connect to the service.
        """
        self._zmq_socket.connect(self._service_address)
        self._log.info(f"Connected to {self._service_name} at {self._service_address}")

    cpdef void disconnect(self) except *:
        """
        Disconnect from the service.
        """
        self._zmq_socket.disconnect(self._service_address)
        self._log.info(f"Disconnected from {self._service_name} at {self._service_address}")

    cpdef void dispose(self) except *:
        """
        Dispose of the MQWorker which close the socket (call disconnect first).
        """
        self._zmq_socket.close()
        self._log.debug(f"Disposed.")

    cpdef bint is_disposed(self):
        """
        Return a value indicating whether the internal socket is disposed.

        :return bool.
        """
        return self._zmq_socket.closed


cdef class RequestWorker(MQWorker):
    """
    Provides a worker for ZMQ requester messaging.
    """

    def __init__(
            self,
            str worker_name,
            str service_name,
            str host,
            int port,
            zmq_context not None: zmq.Context,
            EncryptionConfig encryption not None,
            Logger logger not None):
        """
        Initializes a new instance of the RequestWorker class.

        :param worker_name: The name of the worker.
        :param service_name: The service name.
        :param host: The service host address.
        :param port: The service port.
        :param zmq_context: The ZeroMQ context.
        :param encryption: The encryption configuration.
        :param logger: The logger for the component.
        :raises ValueError: If the worker_name is not a valid string.
        :raises ValueError: If the service_name is not a valid string.
        :raises ValueError: If the service_address is not a valid string.
        :raises ValueError: If the service_port is not in range [0, 65535].
        """
        Condition.valid_string(worker_name, 'worker_name')
        Condition.valid_string(service_name, 'service_name')
        Condition.valid_string(host, 'host')
        Condition.valid_port(port, 'port')
        Condition.type(zmq_context, zmq.Context, 'zmq_context')
        super().__init__(
            worker_name,
            service_name,
            host,
            port,
            zmq_context,
            zmq.REQ,
            encryption,
            logger)

    cpdef bytes send(self, bytes request):
        """
        Send the given request message to the service socket.
        Return the response.

        :param request: The request message bytes to send.
        :return bytes.
        """
        Condition.not_empty(request, 'request')

        self._cycles += 1
        self._zmq_socket.send(request)
        self._log.verbose(f"[{self._cycles}]--> Request of {len(request)} bytes.")

        cdef bytes response
        try:
            response = self._zmq_socket.recv(flags=0)  # None blocking
        except zmq.ZMQError as ex:
            self._log.error(str(ex))
            return None

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
            str host,
            int port,
            zmq_context not None: zmq.Context,
            handler not None: callable,
            EncryptionConfig encryption not None,
            Logger logger not None):
        """
        Initializes a new instance of the SubscriberWorker class.

        :param worker_name: The name of the worker.
        :param service_name: The service name.
        :param host: The service host address.
        :param port: The service port.
        :param zmq_context: The ZeroMQ context.
        :param handler: The message handler.
        :param encryption: The encryption configuration.
        :param logger: The logger for the component.
        :raises ValueError: If the worker_name is not a valid string.
        :raises ValueError: If the service_name is not a valid string.
        :raises ValueError: If the port is not in range [0, 65535].
        :raises ValueError: If the topic is not a valid string.
        :raises TypeError: If the handler is not of type callable.
        """
        Condition.valid_string(worker_name, 'worker_name')
        Condition.valid_string(service_name, 'service_name')
        Condition.valid_string(host, 'host')
        Condition.valid_port(port, 'port')
        Condition.type(zmq_context, zmq.Context, 'zmq_context')
        Condition.callable(handler, 'handler')
        super().__init__(
            worker_name,
            service_name,
            host,
            port,
            zmq_context,
            zmq.SUB,
            encryption,
            logger)

        self._handler = handler
        self._thread = threading.Thread(target=self._consume_messages, daemon=True)
        self._thread.start()

    cpdef void subscribe(self, str topic) except *:
        """
        Subscribe the worker to the given topic.
        
        :param topic: The topic to subscribe to.
        """
        Condition.valid_string(topic, 'topic')

        self._zmq_socket.setsockopt(zmq.SUBSCRIBE, topic.encode(_UTF8))
        self._log.debug(f"Subscribed to topic {topic}.")

    cpdef void unsubscribe(self, str topic) except *:
        """
        Unsubscribe the worker from the given topic.
        
        :param topic: The topic to unsubscribe from.
        """
        Condition.valid_string(topic, 'topic')

        self._zmq_socket.setsockopt(zmq.UNSUBSCRIBE, topic.encode(_UTF8))
        self._log.debug(f"Unsubscribed from topic {topic}.")

    cpdef void _consume_messages(self) except *:
        self._log.info("Running...")

        cdef str topic
        cdef bytes body
        while True:
            self._cycles += 1
            topic = self._zmq_socket.recv().decode(_UTF8)
            body = self._zmq_socket.recv()

            self._log.verbose(f"[{self._cycles}]<-- topic={topic}, message={body}")
            self._handler(topic, body)
