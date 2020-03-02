# -------------------------------------------------------------------------------------------------
# <copyright file="node_servers.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import zmq
import zmq.auth

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Message, MessageType, message_type_to_string, message_type_from_string
from nautilus_trader.core.types cimport GUID
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.guid cimport GuidFactory
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.network.compression cimport Compressor
from nautilus_trader.network.encryption cimport EncryptionSettings
from nautilus_trader.network.identifiers cimport ClientId, ServerId, SessionId
from nautilus_trader.network.queue cimport MessageQueueDuplex, MessageQueueOutbound
from nautilus_trader.network.messages cimport Request, Response, MessageReceived, MessageRejected
from nautilus_trader.network.messages cimport Connect, Connected, Disconnect, Disconnected
from nautilus_trader.serialization.constants cimport *
from nautilus_trader.serialization.constants cimport UTF8

cdef bytes _STRING = message_type_to_string(MessageType.STRING).encode(UTF8)


cdef class ServerNode(NetworkNode):
    """
    The base class for all client nodes.
    """

    def __init__(
            self,
            ServerId server_id not None,
            int port,
            int zmq_socket_type,
            Compressor compressor not None,
            EncryptionSettings encryption not None,
            Clock clock not None,
            GuidFactory guid_factory not None,
            Logger logger not None):
        """
        Initializes a new instance of the ServerNode class.

        :param server_id: The server identifier.
        :param port: The server port.
        :param zmq_socket_type: The ZeroMQ socket type.
        :param compressor: The message compressor.
        :param encryption: The encryption configuration.
        :param clock: The clock for the component.
        :param guid_factory: The guid factory for the component.
        :param logger: The logger for the component.
        :raises ValueError: If the expected frames is negative (< 0).
        :raises ValueError: If the host is not a valid string.
        :raises ValueError: If the port is not in range [0, 65535].
        """
        super().__init__(
            '127.0.0.1',
            port,
            zmq_socket_type,
            compressor,
            encryption,
            clock,
            guid_factory,
            logger)

        self.server_id = server_id
        self._socket.setsockopt(zmq.IDENTITY, self.server_id.value.encode(UTF8))  # noqa (zmq reference)

    cpdef void start(self) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void stop(self) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void _bind_socket(self) except *:
        """
        Connect to the ZMQ socket.
        """
        self._socket.bind(self._network_address)
        self._log.info(f"Bound socket to {self._network_address}")

    cpdef void _unbind_socket(self) except *:
        """
        Disconnect from the ZMQ socket.
        """
        self._socket.unbind(self._network_address)
        self._log.info(f"Unbound socket at {self._network_address}")


cdef class MessageServer(ServerNode):
    """
    Provides an asynchronous messaging server.
    """

    def __init__(
            self,
            ServerId server_id,
            int port,
            DictionarySerializer header_serializer not None,
            RequestSerializer request_serializer not None,
            ResponseSerializer response_serializer not None,
            Compressor compressor not None,
            EncryptionSettings encryption not None,
            Clock clock not None,
            GuidFactory guid_factory not None,
            Logger logger not None):
        """
        Initializes a new instance of the MessageServer class.

        :param server_id: The server identifier.
        :param port: The server port.
        :param request_serializer: The request serializer.
        :param response_serializer: The response serializer.
        :param compressor: The message compressor.
        :param encryption: The encryption configuration.
        :param clock: The clock for the component.
        :param guid_factory: The guid factory for the component.
        :param logger: The logger for the component.
        """
        super().__init__(
            server_id,
            port,
            zmq.ROUTER,  # noqa (zmq reference)
            compressor,
            encryption,
            clock,
            guid_factory,
            logger)

        expected_frames = 3 # [sender, header, body]
        self._queue = MessageQueueDuplex(
            expected_frames,
            self._socket,
            self._handle_frames,
            self._log)

        self._header_serializer = header_serializer
        self._request_serializer = request_serializer
        self._response_serializer = response_serializer
        self._peers = {}    # type: {ClientId, SessionId}
        self._handlers = {} # type: {MessageType, callable}

    cpdef void start(self) except *:
        """
        Start the server.
        """
        self._bind_socket()

    cpdef void stop(self) except *:
        """
        Stop the server.
        """
        self._unbind_socket()

    cpdef void register_request_handler(self, handler) except *:
        """
        Register a request handler which will receive Request messages other 
        than Connect and Disconnect.
        
        Parameters
        ----------
        handler : callable
            The handler to register.
        """
        self._handlers[MessageType.REQUEST] = handler

    cpdef void register_handler(self, MessageType message_type, handler: callable) except *:
        """
        Register a message handler which will receive a list of bytes frames for
        the given message type.

        Parameters
        ----------
        message_type : MessageType
            The message type to register.
        handler : callable
            The handler to register.
        """
        Condition.callable(handler, 'handler')

        if message_type in self._handlers:
            self._log.error(f"A handler for {message_type_to_string(message_type)} was already registered.")
            return

        self._handlers[message_type] = handler

    cpdef void send_rejected(self, str rejected_message, GUID correlation_id, ClientId receiver) except *:
        """
        Send a MessageRejected response.
        
        Parameters
        ----------
        rejected_message : str
            The rejected reason message.
        correlation_id : GUID
            The identifier of the rejected message.
        receiver : ClientId
            The client to send the response to.
        """
        cdef MessageRejected response = MessageRejected(
            rejected_message,
            correlation_id,
            self._guid_factory.generate(),
            self._clock.time_now())

        self.send_response(response, receiver)

    cpdef void send_received(self, Message original, ClientId receiver) except *:
        """
        Send a MessageReceived response for the given original message.
        
        Parameters
        ----------
        original : Request
            The original message received.
        receiver : ClientId
            The client to send the response to.
        """
        cdef MessageReceived response = MessageReceived(
            original.__class__.__name__,
            original.id,
            self._guid_factory.generate(),
            self._clock.time_now())

        self.send_response(response, receiver)

    cpdef void send_response(self, Response response, ClientId receiver) except *:
        """
        Send the given response to the given receiver.
        
        Parameters
        ----------
        response : Response
            The response to send.
        receiver : ClientId
            The response receiver.
        """
        cdef dict header = {
            MESSAGE_TYPE: message_type_to_string(response.message_type),
            TYPE_NAME: response.__class__.__name__
        }

        self._send(receiver, header, self._response_serializer.serialize(response))

    cpdef void send_string(self, str message, ClientId receiver) except *:
        """
        Send the given string message to the given receiver.
        
        Parameters
        ----------
        message : str
            The string message to send. 
        receiver : ClientId
            The message receiver.
        """
        cdef dict header = {
            MESSAGE_TYPE: _STRING,
            TYPE_NAME: UTF8
        }

        self._send(receiver, header, message.encode(UTF8))

    cdef void _send(self, ClientId receiver, dict header, bytes body) except *:
        # Encode and compress frames
        cdef bytes frame_receiver = receiver.value.encode(UTF8)
        cdef bytes frame_header = self._compressor.compress(self._header_serializer.serialize(header))
        cdef bytes frame_body = self._compressor.compress(body)

        self._queue.send([frame_receiver, frame_header, frame_body])
        self._log.verbose(f"[{self.sent_count}]--> header={header}, body={len(frame_body)} bytes")
        self.sent_count += 1

    cpdef void _handle_frames(self, list frames) except *:
        self.recv_count += 1

        # Decompress and decode frames
        cdef bytes sender = frames[0]
        cdef bytes frame_header = self._compressor.decompress(frames[1])
        cdef bytes frame_body = self._compressor.decompress(frames[2])

        cdef ClientId client_id = ClientId(sender.decode(UTF8))
        cdef dict header = self._header_serializer.deserialize(frame_header)

        self._log.verbose(f"<--[{self.recv_count}] header={header}, body={len(frame_body)} bytes")

        cdef MessageType message_type = message_type_from_string(header[MESSAGE_TYPE])
        if message_type == MessageType.STRING:
            handler = self._handlers.get(message_type)
            message = frame_body.decode(UTF8)
            if handler is not None:
                handler(message)
                self._log.verbose(f"<--[{self.recv_count}] '{message}'")
                self.send_string('OK', client_id)
            else:
                self._log.error(f"<--[{self.recv_count}] {message}, with no string handler.")
        elif message_type == MessageType.REQUEST:
            self._handle_request(frame_body, client_id)
        else:
            handler = self._handlers.get(message_type)
            if handler is not None:
                handler(frame_body)

    cdef void _handle_request(self, bytes payload, ClientId sender) except *:
        cdef Request request = self._request_serializer.deserialize(payload)
        self._log.debug(f"<--[{self.sent_count}] {request}")

        if isinstance(request, Connect):
            self._handle_connection(request)
        elif isinstance(request, Disconnect):
            self._handle_disconnection(request)
        else:
            handler = self._handlers.get(MessageType.REQUEST)
            if handler is not None:
                handler(request)

    cdef void _handle_connection(self, Connect request) except *:
        cdef ClientId client_id = request.client_id
        cdef SessionId session_id = self._peers.get(client_id)
        cdef str message
        if session_id is None:
            # Peer not previously connected to a session
            session_id = SessionId(request.authentication)
            self._peers[client_id] = session_id
            message = f"{request.client_id.value} connected to session {session_id.value} at {self._network_address}"
            self._log.info(message)
        else:
            # Peer already connected to a session
            message = f"{request.client_id.value} already connected to session {session_id.value} at {self._network_address}"
            self._log.warning(message)

        cdef Connected response = Connected(
            message,
            self.server_id,
            session_id,
            request.id,
            self._guid_factory.generate(),
            self._clock.time_now())

        self.send_response(response, client_id)

    cdef void _handle_disconnection(self, Disconnect request) except *:
        cdef ClientId client_id = request.client_id
        cdef SessionId session_id = self._peers.get(client_id)
        cdef str message
        if session_id is None:
            # Peer not previously connected to a session
            session_id = SessionId(str(None))
            message = f"{request.client_id.value} had no session to disconnect at {self._network_address}"
            self._log.warning(message)
        else:
            # Peer connected to session
            del self._peers[client_id]
            message = f"{request.client_id.value} disconnected from {session_id.value} at {self._network_address}"
            self._log.info(message)

        cdef Disconnected response = Disconnected(
            message,
            self.server_id,
            session_id,
            request.id,
            self._guid_factory.generate(),
            self._clock.time_now())

        self.send_response(response, client_id)


cdef class MessagePublisher(ServerNode):
    """
    Provides an asynchronous messaging publisher.
    """

    def __init__(self,
                 ServerId server_id,
                 int port,
                 Compressor compressor not None,
                 EncryptionSettings encryption not None,
                 Clock clock not None,
                 GuidFactory guid_factory not None,
                 Logger logger not None):
        """
        Initializes a new instance of the MessagePublisher class.

        :param server_id: The server identifier.
        :param port: The server port.
        :param compressor: The message compressor.
        :param encryption: The encryption configuration.
        :param clock: The clock for the component.
        :param guid_factory: The guid factory for the component.
        :param logger: The logger for the component.
        """
        super().__init__(
            server_id,
            port,
            zmq.PUB,
            compressor,
            encryption,
            clock,
            guid_factory,
            logger)

        self._queue = MessageQueueOutbound(self._socket, self._log)

    cpdef void start(self) except *:
        """
        Stop the server.
        """
        self._bind_socket()

    cpdef void stop(self) except *:
        """
        Stop the server.
        """
        self._unbind_socket()

    cpdef void publish(self, str topic, bytes message) except *:
        """
        Publish the message to subscribers.

        :param topic: The topic of the message being published.
        :param message: The message bytes to send.
        """
        cdef bytes body = self._compressor.compress(message)

        self._log.verbose(f"[{self.sent_count}]--> topic={topic}, body={len(body)} bytes")

        self._queue.send([topic.encode(UTF8), body])
        self.sent_count += 1
