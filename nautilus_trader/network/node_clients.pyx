# -------------------------------------------------------------------------------------------------
# <copyright file="node_clients.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import threading
import zmq
import zmq.auth
from cpython.datetime cimport datetime, timedelta

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.types cimport GUID, Label
from nautilus_trader.core.message cimport Message, MessageType
from nautilus_trader.core.message cimport message_type_to_string, message_type_from_string
from nautilus_trader.common.clock cimport Clock, TimeEvent
from nautilus_trader.common.guid cimport GuidFactory
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.network.compression cimport Compressor
from nautilus_trader.network.encryption cimport EncryptionConfig
from nautilus_trader.network.messages cimport Connect, Connected, Disconnect, Disconnected
from nautilus_trader.network.messages cimport Request, Response

cdef str _UTF8 = 'utf-8'
cdef str _IS_CONNECTED = 'is_connected?'
cdef str _IS_DISCONNECTED = 'is_disconnected?'


cdef class ClientNode(NetworkNode):
    """
    The base class for all client nodes.
    """

    def __init__(
            self,
            ClientId client_id not None,
            str host not None,
            int port,
            int expected_frames,
            zmq_context not None: zmq.Context,
            int zmq_socket_type,
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
        :param expected_frames: The expected message frame count.
        :param zmq_context: The ZeroMQ context.
        :param zmq_socket_type: The ZeroMQ socket type.
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
        super().__init__(
            host,
            port,
            expected_frames,
            zmq_context,
            zmq_socket_type,
            compressor,
            encryption,
            clock,
            guid_factory,
            logger)

        self.client_id = client_id
        self._socket.setsockopt(zmq.IDENTITY, self.client_id.value.encode(_UTF8))  # noqa (zmq reference)

        self._frames_handler = frames_handler
        self._thread = threading.Thread(target=self._consume_messages, daemon=True)
        self._thread.start()

    cpdef bint is_connected(self):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void connect(self) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void disconnect(self) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void _handle_frames(self, list frames) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void _connect_socket(self) except *:
        """
        Connect to the ZMQ socket.
        """
        self._log.info(f"Connecting to {self._network_address}...")
        self._socket.connect(self._network_address)

    cpdef void _disconnect_socket(self) except *:
        """
        Disconnect from the ZMQ socket.
        """
        self._socket.disconnect(self._network_address)
        self._log.info(f"Disconnected from {self._network_address}")

    cpdef void _consume_messages(self) except *:
        self._log.debug("Message consumption loop starting...")

        while True:
            try:
                self._frames_handler(self._socket.recv_multipart(flags=0)) # Blocking per message
                self.recv_count += 1
            except zmq.ZMQError as ex:
                self._log.error(str(ex))
                continue


cdef class MessageClient(ClientNode):
    """
    Provides an asynchronous messaging client.
    """

    def __init__(
            self,
            ClientId client_id not None,
            str host not None,
            int port,
            int expected_frames,
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
        :param host: The server host address.
        :param port: The server port.
        :param expected_frames: The expected message frame count.
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
            expected_frames,
            zmq_context,
            zmq.DEALER,  # noqa (zmq reference)
            self._handle_frames,
            compressor,
            encryption,
            clock,
            guid_factory,
            logger)

        self._request_serializer = request_serializer
        self._response_serializer = response_serializer
        self._response_handler = response_handler
        self._awaiting_reply = {}  # type: {GUID, Message}

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

        cdef datetime timestamp = self._clock.time_now()

        cdef Connect connect = Connect(
            self.client_id,
            SessionId.create(self.client_id, timestamp, 'None').value,
            self._guid_factory.generate(),
            timestamp)

        # Set check connected alert
        self._clock.set_time_alert(Label(_IS_CONNECTED), timestamp + timedelta(seconds=2), self._check_connection)

        self.send_message(connect, self._request_serializer.serialize(connect))

    cpdef void disconnect(self) except *:
        """
        Disconnect from the server.
        """
        if not self.is_connected():
            self._log.warning("No session to disconnect from.")
            return # TODO: Assess how this works

        cdef datetime timestamp = self._clock.time_now()

        cdef Disconnect disconnect = Disconnect(
            self.client_id,
            self.session_id,
            self._guid_factory.generate(),
            timestamp)

        # Set check disconnected alert
        self._clock.set_time_alert(Label(_IS_DISCONNECTED), timestamp + timedelta(seconds=2), self._check_connection)

        self.send_message(disconnect, self._request_serializer.serialize(disconnect))

    cpdef void send_request(self, Request request) except *:
        """
        Send the given request.
        
        Parameters
        ----------
        request : Request
            The request to send.
        """
        self.send_message(request, self._request_serializer.serialize(request))

    cpdef void send_string(self, str message) except *:
        """
        Send the given string message. Note that a reply will not be awaited as
        there is no correlation identifier.
        
        Parameters
        ----------
        message : str
        """
        self.send(MessageType.STRING, message.encode(_UTF8))

    cpdef void send_message(self, Message message, bytes serialized) except *:
        """
        Send the given message which will become durable and await a reply.
        
        Parameters
        ----------
        message : Message
            The message to send.
        serialized : bytes
            The serialized message.
        """
        self._register_message(message)

        self._log.debug(f"[{self.sent_count}]--> {message}")

        self.send(message.message_type, serialized)

    cpdef void send(self, MessageType message_type, bytes serialized) except *:
        """
        Send the given message to the server. 

        :param message_type: The message to send.
        :param serialized: The serialized message.
        """
        Condition.not_empty(serialized, 'payload')

        cdef str send_type_str = message_type_to_string(message_type)
        cdef int send_size = (len(serialized))

        # Encode frames
        cdef bytes header_type = send_type_str.encode(_UTF8)
        cdef bytes header_size = str(send_size).encode(_UTF8)
        cdef bytes payload = self._compressor.compress(serialized)

        self._log.verbose(f"[{self.sent_count}]--> "
                          f"type={send_type_str}, "
                          f"size={send_size} bytes,"
                          f"payload={len(payload)} bytes")

        self._send([header_type, header_size, payload])

    cpdef void _handle_frames(self, list frames) except *:
        cdef int frames_count = len(frames)
        if frames_count != self._expected_frames:
            self._log.error(f"Received unexpected frames count {frames_count}, expected {self._expected_frames}")
            return

        cdef str header_type = frames[0].decode(_UTF8)
        cdef int header_size = int(frames[1].decode(_UTF8))
        cdef bytes payload = self._compressor.decompress(frames[2])

        cdef MessageType message_type = message_type_from_string(header_type)
        if message_type == MessageType.STRING:
            message = payload.decode(_UTF8)
            self._log.verbose(f"<--[{self.recv_count}] '{message}'")
            self._response_handler(message)
            return

        self._log.verbose(f"<--[{self.recv_count}] "
                          f"type={header_type}, "
                          f"size={header_size} bytes, "
                          f"payload={len(payload)} bytes")

        if message_type != MessageType.RESPONSE:
            self._log.error(f"Not a valid response, was {header_type}")
            return

        cdef Response response = self._response_serializer.deserialize(payload)
        self._log.debug(f"<--[{self.sent_count}] {response}")
        self._deregister_message(response.correlation_id)

        if isinstance(response, Connected):
            if self.session_id is not None:
                self._log.warning(response.message)
            else:
                self._log.info(response.message)
            self.session_id = response.session_id
            return
        elif isinstance(response, Disconnected):
            if self.session_id is None:
                self._log.warning(response.message)
            else:
                self._log.info(response.message)
            self.session_id = None
            self._disconnect_socket()
        else:
            self._response_handler(response)

    cpdef void _check_connection(self, TimeEvent event):
        if event.label == _IS_CONNECTED:
            if not self.is_connected():
                self._log.warning("Connection timed out...")
        elif event.label == _IS_DISCONNECTED:
            if self.is_connected():
                self._log.warning("Still connected...")
        else:
            self._log.error(f"Check connection message '{event.label}' not recognized.")

    cdef void _register_message(self, Message message, int retry=0):
        try:
            if retry < 3:
                self._awaiting_reply[message.id] = message
                self._log.verbose(f"Registered message with id {message.id.value} to await reply.")
            else:
                self._log.error(f"Could not register {message} to await reply, retries={retry}.")
        except RuntimeError as ex:
            retry += 1
            self._register_message(message, retry)

    cdef void _deregister_message(self, GUID correlation_id, int retry=0):
        cdef Message message
        try:
            if retry < 3:
                message = self._awaiting_reply.pop(correlation_id)
                if message is None:
                    self._log.error(f"No awaiting message for correlation id {correlation_id.value}.")
                else:
                    self._log.verbose(f"Received reply for message with id {message.id.value}.")
                    pass
            else:
                self._log.error(f"Could not deregister with correlation id {correlation_id.value}, retries={retry}.")
        except RuntimeError as ex:
            retry += 1
            self._deregister_message(message, retry)


cdef class MessageSubscriber(ClientNode):
    """
    Provides an asynchronous messaging subscriber.
    """

    def __init__(
            self,
            ClientId client_id,
            str host,
            int port,
            int expected_frames,
            zmq_context not None: zmq.Context,
            subscription_handler not None: callable,
            Compressor compressor not None,
            EncryptionConfig encryption not None,
            Clock clock not None,
            GuidFactory guid_factory not None,
            Logger logger not None):
        """
        Initializes a new instance of the SubscriberWorker class.

        :param client_id: The client identifier for the worker.
        :param host: The service host address.
        :param port: The service port.
        :param expected_frames: The expected message frame count.
        :param zmq_context: The ZeroMQ context.
        :param subscription_handler: The message handler.
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
        Condition.valid_string(host, 'host')
        Condition.valid_port(port, 'port')
        Condition.type(zmq_context, zmq.Context, 'zmq_context')
        Condition.callable(subscription_handler, 'handler')
        super().__init__(
            client_id,
            host,
            port,
            zmq_context,
            zmq.SUB,
            expected_frames,
            self._handle_frames,
            compressor,
            encryption,
            clock,
            guid_factory,
            logger)

        self._subscription_handler = subscription_handler

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

        self._socket.setsockopt(zmq.SUBSCRIBE, topic.encode(_UTF8))
        self._log.debug(f"Subscribed to topic {topic}")

    cpdef void unsubscribe(self, str topic) except *:
        """
        Unsubscribe the worker from the given topic.
        
        :param topic: The topic to unsubscribe from.
        """
        Condition.valid_string(topic, 'topic')

        self._socket.setsockopt(zmq.UNSUBSCRIBE, topic.encode(_UTF8))
        self._log.debug(f"Unsubscribed from topic {topic}")

    cpdef void _handle_frames(self, list frames) except *:
        self.recv_count += 1

        cdef int frames_count = len(frames)
        if frames_count != self._expected_frames:
            self._log.error(f"Message was malformed (expected {self._expected_frames} frames, received {frames_count}).")
            return

        cdef str recv_topic = frames[0].decode(_UTF8)
        cdef int recv_size = int.from_bytes(frames[1], byteorder='big', signed=True)
        cdef bytes payload = self._compressor.decompress(frames[2])

        self._subscription_handler(recv_topic, payload)
