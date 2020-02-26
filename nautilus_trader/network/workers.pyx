# -------------------------------------------------------------------------------------------------
# <copyright file="workers.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import os
import time
import threading
import zmq
import zmq.auth

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport MessageType, message_type_to_string, message_type_from_string
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.guid cimport GuidFactory
from nautilus_trader.common.logger cimport Logger, LoggerAdapter
from nautilus_trader.network.compression cimport Compressor
from nautilus_trader.network.encryption cimport EncryptionConfig
from nautilus_trader.network.messages cimport Connect, Disconnect, Response

cdef str _UTF8 = 'utf-8'


cdef class MQWorker:
    """
    The base class for all ZMQ messaging workers.
    """

    def __init__(
            self,
            ClientId client_id not None,
            str host not None,
            int port,
            zmq_context not None: zmq.Context,
            int zmq_socket_type,
            int expected_frames,
            frames_handler not None: callable,
            Compressor compressor not None,
            EncryptionConfig encryption not None,
            Clock clock not None,
            GuidFactory guid_factory not None,
            Logger logger not None):
        """
        Initializes a new instance of the MQWorker class.

        :param client_id: The client identifier for the worker.
        :param host: The service host address.
        :param port: The service port.
        :param zmq_context: The ZeroMQ context.
        :param zmq_socket_type: The ZeroMQ socket type.
        :param expected_frames: The expected received frame count.
        :param frames_handler: The frames handler.
        :param compressor: The message compressor.
        :param encryption: The encryption configuration.
        :param clock: The clock for the component.
        :param guid_factory: The guid factory for the component.
        :param logger: The logger for the component.
        :raises ValueError: If the expected frames is not positive (> 0).
        :raises ValueError: If the host is not a valid string.
        :raises ValueError: If the port is not in range [0, 65535].
        """
        Condition.positive(expected_frames, 'expected_frames')
        Condition.valid_string(host, 'host')
        Condition.valid_port(port, 'port')
        Condition.type(zmq_context, zmq.Context, 'zmq_context')

        self._clock = clock
        self._guid_factory = guid_factory
        self._log = LoggerAdapter(client_id.value, logger)
        self._server_address = f'tcp://{host}:{port}'
        self._zmq_context = zmq_context
        self._zmq_socket = self._zmq_context.socket(zmq_socket_type)
        self._zmq_socket.setsockopt(zmq.LINGER, 1)
        self._expected_frames = expected_frames
        self._compressor = compressor
        self._cycles = 0

        self.client_id = client_id

        if encryption.use_encryption:
            if encryption.algorithm != 'curve':
                raise ValueError(f'Invalid encryption specified, was \'{encryption.algorithm}\'')
            key_file_client = os.path.join(encryption.keys_dir, "client.key_secret")
            key_file_server = os.path.join(encryption.keys_dir, "server.key")
            client_public, client_secret = zmq.auth.load_certificate(key_file_client)
            server_public, server_secret = zmq.auth.load_certificate(key_file_server)
            self._zmq_socket.curve_secretkey = client_secret
            self._zmq_socket.curve_publickey = client_public
            self._zmq_socket.curve_serverkey = server_public
            self._log.info(f"Curve25519 encryption setup for {self._server_address}")
        else:
            self._log.warning(f"No encryption setup for {self._server_address}")

        self._frames_handler = frames_handler
        self._thread = threading.Thread(target=self._consume_messages, daemon=True)
        self._thread.start()

    cpdef bint is_connected(self):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef bint is_disposed(self):
        """
         Return a value indicating whether the internal socket is disposed.
    
        :return bool.
        """
        return self._zmq_socket.closed

    cpdef void connect(self) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void disconnect(self) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void _handle_frames(self, list frames) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void dispose(self) except *:
        """
        Dispose of the MQWorker which close the socket (call disconnect first).
        """
        self._zmq_socket.close()
        self._log.debug(f"Disposed.")

    cpdef void _connect_socket(self) except *:
        """
        Connect to the ZMQ socket.
        """
        self._log.info(f"Connecting to {self._server_address}...")
        self._zmq_socket.connect(self._server_address)

    cpdef void _disconnect_socket(self) except *:
        """
        Disconnect from the ZMQ socket.
        """
        self._zmq_socket.disconnect(self._server_address)
        self._log.info(f"Disconnected from {self._server_address}")

    cpdef void _consume_messages(self) except *:
        self._log.info("Ready to consume messages...")

        while True:
            self._cycles += 1

            try:
                self._frames_handler(self._zmq_socket.recv_multipart(flags=0)) # Blocking per message
            except zmq.ZMQError as ex:
                self._log.error(str(ex))
                return


cdef class DealerWorker(MQWorker):
    """
    Provides an asynchronous worker for ZMQ dealer messaging.
    """

    def __init__(
            self,
            ClientId client_id not None,
            str host not None,
            int port,
            zmq_context not None: zmq.Context,
            response_handler not None: callable,
            RequestSerializer request_serializer not None,
            ResponseSerializer response_serializer not None,
            Compressor compressor not None,
            EncryptionConfig encryption not None,
            Clock clock not None,
            GuidFactory guid_factory not None,
            Logger logger not None):
        """
        Initializes a new instance of the DealerWorker class.

        :param client_id: The client identifier for the worker.
        :param host: The service host address.
        :param port: The service port.
        :param zmq_context: The ZeroMQ context.
        :param response_handler: The handler for response messages.
        :param request_serializer: The request serializer.
        :param response_serializer: The response serializer.
        :param compressor: The message compressor.
        :param encryption: The encryption configuration.
        :param clock: The clock for the component.
        :param guid_factory: The guid factory for the component.
        :param logger: The logger for the component.
        :raises ValueError: If the host is not a valid string.
        :raises ValueError: If the port is not in range [0, 65535].
        """
        Condition.valid_string(host, 'host')
        Condition.valid_port(port, 'port')
        Condition.type(zmq_context, zmq.Context, 'zmq_context')
        super().__init__(
            client_id,
            host,
            port,
            zmq_context,
            zmq.REQ,
            4,
            self._handle_frames,
            compressor,
            encryption,
            clock,
            guid_factory,
            logger)

        self._request_serializer = request_serializer
        self._response_serializer = response_serializer
        self._response_handler = response_handler

    cpdef bint is_connected(self):
        """
        Return a value indicating whether the client is connected to the server.
        """
        return self.session_id is not None

    cpdef void connect(self) except *:
        """
        Connect to the server.
        """
        self._connect_socket()
        time.sleep(0.1) # TODO: Temporary delay

        cdef Connect connect = Connect(
            self.client_id,
            self._guid_factory.generate(),
            self._clock.time_now())

        self.send(connect.message_type, self._request_serializer.serialize(connect))

    cpdef void disconnect(self) except *:
        """
        Disconnect from the server.
        """
        if not self.is_connected():
            self._log.warning("No session to disconnect from.")
            return # TODO: Assess how this works

        cdef Disconnect disconnect = Disconnect(
            self.client_id,
            self.session_id,
            self._guid_factory.generate(),
            self._clock.time_now())

        self.send(disconnect.message_type, self._request_serializer.serialize(disconnect))

    cpdef void send(self, MessageType message_type, bytes payload) except *:
        """
        Send the given request message to the service socket.
        Return the response.

        :param message_type: The message type to send.
        :param payload: The payload to send.
        """
        Condition.not_equal(message_type, MessageType.UNDEFINED, 'message_type', 'UNDEFINED')
        Condition.not_empty(payload, 'payload')

        self._cycles += 1

        cdef str send_type_str = message_type_to_string(message_type)
        cdef int send_size = (len(payload))

        # Encode frames
        cdef bytes header_type = send_type_str.encode(_UTF8)
        cdef bytes header_size = bytes([send_size])
        cdef bytes compressed = self._compressor.compress(payload)

        self._zmq_socket.send_multipart(header_type, header_size, compressed)
        self._log.verbose(f"[{self._cycles}]--> {send_type_str} of {send_size} bytes.")

    cpdef void _handle_frames(self, list frames) except *:
        cdef int frames_count = len(frames)
        if frames_count != self._expected_frames:
            self._log.error("Received unexpected frames count")
            return

        cdef str recv_type = frames[0].decode(_UTF8)
        cdef int recv_size = int.from_bytes(frames[1], byteorder='big', signed=True)
        cdef bytes payload = self._compressor.decompress(frames[2])

        cdef MessageType message_type = message_type_from_string(recv_type)
        if message_type != MessageType.RESPONSE:
            self._log.error(f"Not a valid response, was {message_type}")

        cdef Response response = self._response_serializer.deserialize(payload)

        self._log.verbose(f"[{self._cycles}]<-- type={recv_type}, size={recv_size} bytes")

        if isinstance(response, Connect):
            if self.session_id is not None:
                self._log.warning(response.message)
            else:
                self._log.info(response.message)
            self.session_id = response
            return
        elif isinstance(response, Disconnect):
            if self.session_id is None:
                self._log.warning(response.message)
            else:
                self._log.info(response.message)
            self.session_id = None
            self._disconnect_socket()
        else:
            self._response_handler(response)


cdef class SubscriberWorker(MQWorker):
    """
    Provides an asynchronous worker for ZMQ subscriber messaging.
    """

    def __init__(
            self,
            ClientId client_id,
            str service_name,
            str host,
            int port,
            zmq_context not None: zmq.Context,
            sub_handler not None: callable,
            Compressor compressor not None,
            EncryptionConfig encryption not None,
            Clock clock not None,
            GuidFactory guid_factory not None,
            Logger logger not None):
        """
        Initializes a new instance of the SubscriberWorker class.

        :param client_id: The client identifier for the worker.
        :param service_name: The service name to connect to.
        :param host: The service host address.
        :param port: The service port.
        :param zmq_context: The ZeroMQ context.
        :param sub_handler: The message handler.
        :param compressor: The The message compressor.
        :param encryption: The encryption configuration.
        :param clock: The clock for the component.
        :param guid_factory: The guid factory for the component.
        :param logger: The logger for the component.
        :raises ValueError: If the service_name is not a valid string.
        :raises ValueError: If the port is not in range [0, 65535].
        :raises ValueError: If the topic is not a valid string.
        :raises TypeError: If the handler is not of type callable.
        """
        Condition.valid_string(service_name, 'service_name')
        Condition.valid_string(host, 'host')
        Condition.valid_port(port, 'port')
        Condition.type(zmq_context, zmq.Context, 'zmq_context')
        Condition.callable(sub_handler, 'handler')
        super().__init__(
            client_id,
            host,
            port,
            zmq_context,
            zmq.SUB,
            3,
            self._handle_frames,
            compressor,
            encryption,
            clock,
            guid_factory,
            logger)

        self.service_name = service_name
        self._sub_handler = sub_handler

    cpdef bint is_connected(self):
        return True # TODO: Keep alive heartbeat polling

    cpdef void connect(self) except *:
        """
        Connect to the publisher.
        """
        self._connect_socket()

    cpdef void disconnect(self) except *:
        """
        Disconnect from the publisher.
        """
        self._disconnect_socket()

    cpdef void subscribe(self, str topic) except *:
        """
        Subscribe the worker to the given topic.
        
        :param topic: The topic to subscribe to.
        """
        Condition.valid_string(topic, 'topic')

        self._zmq_socket.setsockopt(zmq.SUBSCRIBE, topic.encode(_UTF8))
        self._log.debug(f"Subscribed to topic {topic} at {self.service_name}")

    cpdef void unsubscribe(self, str topic) except *:
        """
        Unsubscribe the worker from the given topic.
        
        :param topic: The topic to unsubscribe from.
        """
        Condition.valid_string(topic, 'topic')

        self._zmq_socket.setsockopt(zmq.UNSUBSCRIBE, topic.encode(_UTF8))
        self._log.debug(f"Unsubscribed from topic {topic} at {self.service_name}")

    cpdef void _handle_frames(self, list frames) except *:
        cdef int frames_count = len(frames)
        if frames_count != self._expected_frames:
            self._log.error(f"Message was malformed (expected {self._expected_frames} frames, received {frames_count}).")
            return

        cdef str recv_topic = frames[0].decode(_UTF8)
        cdef int recv_size = int.from_bytes(frames[1], byteorder='big', signed=True)
        cdef bytes payload = self._compressor.decompress(frames[2])

        self._sub_handler(recv_topic, payload)
