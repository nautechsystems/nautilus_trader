# -------------------------------------------------------------------------------------------------
# <copyright file="node_clients.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import zmq
from cpython.datetime cimport datetime, timedelta

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.types cimport GUID, Label
from nautilus_trader.core.message cimport Message, MessageType
from nautilus_trader.core.message cimport message_type_to_string, message_type_from_string
from nautilus_trader.common.clock cimport Clock, TimeEvent
from nautilus_trader.common.guid cimport GuidFactory
from nautilus_trader.network.compression cimport Compressor
from nautilus_trader.network.encryption cimport EncryptionSettings
from nautilus_trader.network.messages cimport Connect, Connected, Disconnect, Disconnected
from nautilus_trader.network.messages cimport Request, Response
from nautilus_trader.network.queue cimport MessageQueueInbound, MessageQueueOutbound
from nautilus_trader.network.socket cimport ClientSocket
from nautilus_trader.serialization.base cimport DictionarySerializer, RequestSerializer, ResponseSerializer
from nautilus_trader.serialization.constants cimport *

cdef str _IS_CONNECTED = 'is_connected?'
cdef str _IS_DISCONNECTED = 'is_disconnected?'


cdef class ClientNode:
    """
    The base class for all client nodes.
    """

    def __init__(
            self,
            ClientId client_id not None,
            Compressor compressor not None,
            Clock clock not None,
            GuidFactory guid_factory not None,
            LoggerAdapter logger not None):
        """
        Initializes a new instance of the ClientNode class.

        :param client_id: The client identifier.
        :param compressor: The message compressor.
        :param clock: The clock for the component.
        :param guid_factory: The guid factory for the component.
        :param logger: The logger for the component.
        :raises ValueError: If the host is not a valid string.
        :raises ValueError: If the port is not in range [49152, 65535].
        """
        self._compressor = compressor
        self._clock = clock
        self._guid_factory = guid_factory
        self._log = logger
        self._message_handler = None

        self.client_id = client_id
        self.sent_count = 0
        self.recv_count = 0

    cpdef void register_handler(self, handler: callable) except *:
        """
        Register a handler to receive messages.

        Parameters
        ----------
        handler : callable
            The handler to register.
        """
        Condition.callable(handler, 'handler')

        if self._message_handler is not None:
            self._log.debug(f"Registered message handler {handler} by replacing {self._message_handler}.")
        else:
            self._log.debug(f"Registered message handler {handler}.")

        self._message_handler = handler

    cpdef bint is_connected(self):
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void connect(self) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void disconnect(self) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void dispose(self) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")


cdef class MessageClient(ClientNode):
    """
    Provides an asynchronous messaging client.
    """

    def __init__(
            self,
            ClientId client_id not None,
            str server_host not None,
            int server_req_port,
            int server_res_port,
            DictionarySerializer header_serializer not None,
            RequestSerializer request_serializer not None,
            ResponseSerializer response_serializer not None,
            Compressor compressor not None,
            EncryptionSettings encryption not None,
            Clock clock not None,
            GuidFactory guid_factory not None,
            LoggerAdapter logger not None):
        """
        Initializes a new instance of the MessageClient class.

        :param client_id: The client identifier for the worker.
        :param server_host: The server host address.
        :param server_req_port: The server request port.
        :param server_res_port: The server response port.
        :param header_serializer: The header serializer.
        :param request_serializer: The request serializer.
        :param response_serializer: The response serializer.
        :param compressor: The message compressor.
        :param encryption: The encryption configuration.
        :param clock: The clock for the component.
        :param guid_factory: The guid factory for the component.
        :param logger: The logger for the component.
        :raises ValueError: If the host is not a valid string.
         :raises ValueError: If the port is not in range [49152, 65535].
        """
        Condition.valid_string(server_host, 'host')
        Condition.valid_port(server_req_port, 'server_in_port')
        Condition.valid_port(server_res_port, 'server_out_port')
        super().__init__(
            client_id,
            compressor,
            clock,
            guid_factory,
            logger)

        self._socket_outbound = ClientSocket(
            client_id,
            server_host,
            server_req_port,
            zmq.DEALER,  # noqa (zmq reference)
            encryption,
            self._log)

        self._socket_inbound = ClientSocket(
            client_id,
            server_host,
            server_res_port,
            zmq.DEALER,  # noqa (zmq reference)
            encryption,
            self._log)

        self._queue_outbound = MessageQueueOutbound(
            self._socket_outbound,
            self._log)

        expected_frames = 2 # [header, body]
        self._queue_inbound = MessageQueueInbound(
            expected_frames,
            self._socket_inbound,
            self._recv_frames,
            self._log)

        self._header_serializer = header_serializer
        self._request_serializer = request_serializer
        self._response_serializer = response_serializer
        self._message_handler = None
        self._awaiting_reply = {}  # type: {GUID, Message}

        self.session_id = None

    cpdef bint is_connected(self):
        """
        Return a value indicating whether the client is connected to the server.
        """
        return self.session_id is not None

    cpdef void connect(self) except *:
        """
        Connect to the server.
        """
        self._socket_outbound.connect()
        self._socket_inbound.connect()

        cdef datetime timestamp = self._clock.time_now()

        cdef Connect connect = Connect(
            self.client_id,
            SessionId.create(self.client_id, timestamp, 'None').value,
            self._guid_factory.generate(),
            timestamp)

        # Set check connected alert
        self._clock.set_time_alert(
            Label(connect.id.value + _IS_CONNECTED),
            timestamp + timedelta(seconds=2),
            self._check_connection)

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
        self._clock.set_time_alert(
            Label(disconnect.id.value + _IS_DISCONNECTED),
            timestamp + timedelta(seconds=2),
            self._check_connection)

        self.send_message(disconnect, self._request_serializer.serialize(disconnect))

    cpdef void dispose(self) except *:
        """
        Dispose of the MQWorker which close the socket (call disconnect first).
        """
        self._socket_outbound.dispose()
        self._socket_inbound.dispose()
        self._log.debug(f"Disposed.")

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
        self._send(MessageType.STRING, UTF8, message.encode(UTF8))

    cpdef void send_message(self, Message message, bytes body) except *:
        """
        Send the given message to the server.

        Parameters
        ----------
        message : Message
            The message to send.
        body : bytes
            The serialized message body.
        """
        self._register_message(message)

        self._log.debug(f"[{self.sent_count}]--> {message}")
        self._send(message.message_type, message.__class__.__name__, body)

    cdef void _send(self, MessageType message_type, str class_name, bytes body) except *:
        """
        Send the given message to the server. 

        Parameters
        ----------
        message_type : MessageType
            The message type group.
        class_name : str
            The message class name.
        body : bytes
            The serialized
        """
        Condition.not_equal(message_type, MessageType.UNDEFINED, 'message_type', 'UNDEFINED')
        Condition.valid_string(class_name, 'class_name')
        Condition.not_empty(body, 'body')

        cdef dict header = {
            MESSAGE_TYPE: message_type_to_string(message_type).title(),
            TYPE: class_name
        }

        # Compress frames
        cdef bytes frame_header = self._compressor.compress(self._header_serializer.serialize(header))
        cdef bytes frame_body = self._compressor.compress(body)

        self._queue_outbound.send([frame_header, frame_body])
        self._log.verbose(f"[{self.sent_count}]--> header={header}, body={len(frame_body)} bytes")
        self.sent_count += 1

    cpdef void _recv_frames(self, list frames) except *:
        self.recv_count += 1

        # Decompress frames
        cdef bytes frame_header = self._compressor.decompress(frames[0])
        cdef bytes frame_body = self._compressor.decompress(frames[1])

        cdef dict header = self._header_serializer.deserialize(frame_header)

        cdef MessageType message_type = message_type_from_string(header[MESSAGE_TYPE].upper())
        if message_type == MessageType.STRING:
            message = frame_body.decode(UTF8)
            self._log.verbose(f"<--[{self.recv_count}] '{message}'")
            if self._message_handler is not None:
                self._message_handler(message)
            return

        self._log.verbose(f"<--[{self.recv_count}] header={header}, body={len(frame_body)} bytes")

        if message_type != MessageType.RESPONSE:
            self._log.error(f"Not a valid response, was {header[MESSAGE_TYPE]}")
            return

        cdef Response response = self._response_serializer.deserialize(frame_body)
        self._log.debug(f"<--[{self.recv_count}] {response}")
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
            self._socket_outbound.disconnect()
            self._socket_inbound.disconnect()
        else:
            if self._message_handler is not None:
                self._message_handler(response)

    cpdef void _check_connection(self, TimeEvent event) except *:
        if event.label.value.endswith(_IS_CONNECTED):
            if not self.is_connected():
                self._log.warning("Connection request timed out...")
        elif event.label.value.endswith(_IS_DISCONNECTED):
            if self.is_connected():
                self._log.warning(f"Session {self.session_id} is still connected...")
        else:
            self._log.error(f"Check connection message '{event.label}' not recognized.")

    cdef void _register_message(self, Message message, int retry=0) except *:
        try:
            if retry < 3:
                self._awaiting_reply[message.id] = message
                self._log.verbose(f"Registered message with id {message.id.value} to await reply.")
            else:
                self._log.error(f"Could not register {message} to await reply, retries={retry}.")
        except RuntimeError as ex:
            retry += 1
            self._register_message(message, retry)

    cdef void _deregister_message(self, GUID correlation_id, int retry=0) except *:
        cdef Message message
        try:
            if retry < 3:
                message = self._awaiting_reply.pop(correlation_id, None)
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
            Compressor compressor not None,
            EncryptionSettings encryption not None,
            Clock clock not None,
            GuidFactory guid_factory not None,
            LoggerAdapter logger):
        """
        Initializes a new instance of the MessageSubscriber class.

        :param client_id: The client identifier for the worker.
        :param host: The service host address.
        :param port: The service port.
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
        super().__init__(
            client_id,
            compressor,
            clock,
            guid_factory,
            logger)

        self.register_handler(self._no_subscriber_handler)

        self._socket = SubscriberSocket(
            client_id,
            host,
            port,
            encryption,
            self._log)

        expected_frames = 2 # [topic, body]
        self._queue = MessageQueueInbound(
            expected_frames,
            self._socket,
            self._recv_frames,
            self._log)

    cpdef bint is_connected(self):
        return True # TODO: Keep alive heartbeat polling

    cpdef void connect(self) except *:
        """
        Connect to the publisher.
        """
        self._socket.connect()

    cpdef void disconnect(self) except *:
        """
        Disconnect from the publisher.
        """
        self._socket.disconnect()

    cpdef void dispose(self) except *:
        """
        Dispose of the MQWorker which close the socket (call disconnect first).
        """
        self._socket.dispose()
        self._log.debug(f"Disposed.")

    cpdef void subscribe(self, str topic) except *:
        """
        Subscribe the worker to the given topic.
        
        :param topic: The topic to subscribe to.
        """
        Condition.valid_string(topic, 'topic')

        self._socket.subscribe(topic)

    cpdef void unsubscribe(self, str topic) except *:
        """
        Unsubscribe the worker from the given topic.
        
        :param topic: The topic to unsubscribe from.
        """
        Condition.valid_string(topic, 'topic')

        self._socket.unsubscribe(topic)

    cpdef void _recv_frames(self, list frames) except *:
        cdef str topic = frames[0].decode(UTF8)
        cdef bytes body = self._compressor.decompress(frames[1])

        self._message_handler(topic, body)
        self.recv_count += 1

    cpdef void _no_subscriber_handler(self, str topic, bytes body) except *:
        self._log.warning(f"Received message from topic {topic} with no handler registered.")
