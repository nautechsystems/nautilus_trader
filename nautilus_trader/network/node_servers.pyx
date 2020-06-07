# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import zmq

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Message, MessageType, message_type_to_string, message_type_from_string
from nautilus_trader.core.types cimport GUID
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.guid cimport GuidFactory
from nautilus_trader.network.compression cimport Compressor
from nautilus_trader.network.encryption cimport EncryptionSettings
from nautilus_trader.network.identifiers cimport ClientId, ServerId, SessionId
from nautilus_trader.network.queue cimport MessageQueueInbound, MessageQueueOutbound
from nautilus_trader.network.messages cimport Request, Response, MessageReceived, MessageRejected
from nautilus_trader.network.messages cimport Connect, Connected, Disconnect, Disconnected
from nautilus_trader.serialization.constants cimport *
from nautilus_trader.serialization.constants cimport UTF8

cdef bytes _STRING = message_type_to_string(MessageType.STRING).title().encode(UTF8)
cdef str _TYPE_UTF8 = 'UTF8'


cdef class ServerNode:
    """
    The base class for all server nodes.
    """

    def __init__(
            self,
            ServerId server_id not None,
            Compressor compressor not None,
            Clock clock not None,
            GuidFactory guid_factory not None,
            LoggerAdapter logger not None):
        """
        Initializes a new instance of the ServerNode class.

        :param server_id: The server identifier.
        :param compressor: The message compressor.
        :param clock: The clock for the component.
        :param guid_factory: The guid factory for the component.
        :param logger: The logger for the component.
        """
        self._compressor = compressor
        self._clock = clock
        self._guid_factory = guid_factory
        self._log = logger

        self.server_id = server_id
        self.sent_count = 0
        self.recv_count = 0

    cpdef void start(self) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void stop(self) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void dispose(self) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")


cdef class MessageServer(ServerNode):
    """
    Provides an asynchronous messaging server.
    """

    def __init__(
            self,
            ServerId server_id,
            int recv_port,
            int send_port,
            DictionarySerializer header_serializer not None,
            RequestSerializer request_serializer not None,
            ResponseSerializer response_serializer not None,
            Compressor compressor not None,
            EncryptionSettings encryption not None,
            Clock clock not None,
            GuidFactory guid_factory not None,
            LoggerAdapter logger not None):
        """
        Initializes a new instance of the MessageServer class.

        :param server_id: The server identifier.
        :param recv_port: The server receive port.
        :param send_port: The server send port.
        :param header_serializer: The header serializer.
        :param request_serializer: The request serializer.
        :param response_serializer: The response serializer.
        :param compressor: The message compressor.
        :param encryption: The encryption configuration.
        :param clock: The clock for the component.
        :param guid_factory: The guid factory for the component.
        :param logger: The logger for the component.
        """
        Condition.valid_port(send_port, 'send_port')
        Condition.valid_port(recv_port, 'recv_port')
        super().__init__(
            server_id,
            compressor,
            clock,
            guid_factory,
            logger)

        self._socket_inbound = ServerSocket(
            server_id,
            recv_port,
            zmq.ROUTER,  # noqa (zmq reference)
            encryption,
            self._log)

        self._socket_outbound = ServerSocket(
            server_id,
            send_port,
            zmq.ROUTER,  # noqa (zmq reference)
            encryption,
            self._log)

        expected_frames = 3 # [sender, header, body]
        self._queue_inbound = MessageQueueInbound(
            expected_frames,
            self._socket_inbound,
            self._recv_frames,
            self._log)

        self._queue_outbound = MessageQueueOutbound(
            self._socket_outbound,
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
        self._socket_inbound.connect()
        self._socket_outbound.connect()

    cpdef void stop(self) except *:
        """
        Stop the server.
        """
        self._socket_inbound.disconnect()
        self._socket_outbound.disconnect()

    cpdef void dispose(self) except *:
        """
        Dispose of the servers sockets (call disconnect first).
        """
        self._socket_inbound.dispose()
        self._socket_outbound.dispose()
        self._log.debug(f"Disposed.")

    cpdef void register_request_handler(self, handler: callable) except *:
        """
        Register a request handler which will receive Request messages other 
        than Connect and Disconnect.
        
        Parameters
        ----------
        handler : callable
            The handler to register.
            
        """
        Condition.callable(handler, 'handler')

        self._handlers[MessageType.REQUEST] = handler

    cpdef void register_handler(self, MessageType message_type, handler: callable) except *:
        """
        Register a message handler which will to receive payloads of the given message type.

        Parameters
        ----------
        message_type : MessageType
            The message type to register.
        handler : callable
            The handler to register.
            
        """
        Condition.not_equal(message_type, MessageType.UNDEFINED, 'message_type', 'UNDEFINED')
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
        Condition.not_none(correlation_id, 'correlation_id')

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
        Condition.not_none(response, 'response')

        cdef dict header = {
            MESSAGE_TYPE: message_type_to_string(response.message_type).title(),
            TYPE: response.__class__.__name__
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
            TYPE: _TYPE_UTF8
        }

        self._send(receiver, header, message.encode(UTF8))

    cdef void _send(self, ClientId receiver, dict header, bytes body) except *:
        Condition.not_none(receiver, 'receiver')
        Condition.not_none(header, 'header')

        # Encode and compress frames
        cdef bytes frame_receiver = receiver.value.encode(UTF8)
        cdef bytes frame_header = self._compressor.compress(self._header_serializer.serialize(header))
        cdef bytes frame_body = self._compressor.compress(body)

        self._queue_outbound.send([frame_receiver, frame_header, frame_body])
        self._log.verbose(f"[{self.sent_count}]--> header={header}, body={len(frame_body)} bytes")
        self.sent_count += 1

    cpdef void _recv_frames(self, list frames) except *:
        self.recv_count += 1

        # Decompress and decode frames
        cdef bytes frame_sender = frames[0]
        cdef bytes frame_header = self._compressor.decompress(frames[1])
        cdef bytes frame_body = self._compressor.decompress(frames[2])

        cdef ClientId client_id = ClientId(frame_sender.decode(UTF8))
        cdef dict header = self._header_serializer.deserialize(frame_header)

        self._log.verbose(f"<--[{self.recv_count}] header={header}, body={len(frame_body)} bytes")

        cdef MessageType message_type = message_type_from_string(header[MESSAGE_TYPE].upper())
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

    cdef void _handle_request(self, bytes body, ClientId sender) except *:
        cdef Request request = self._request_serializer.deserialize(body)
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
            message = f"{request.client_id.value} connected to session {session_id.value} with {self.server_id.value}"
            self._log.info(message)
        else:
            # Peer already connected to a session
            message = f"{request.client_id.value} already connected to session {session_id.value} with {self.server_id.value}"
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
            message = f"{request.client_id.value} had no session to disconnect with {self.server_id.value}"
            self._log.warning(message)
        else:
            # Peer connected to session
            del self._peers[client_id]
            message = f"{request.client_id.value} disconnected from {session_id.value} with {self.server_id.value}"
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
                 LoggerAdapter logger not None):
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
            compressor,
            clock,
            guid_factory,
            logger)

        self._socket = ServerSocket(
            server_id,
            port,
            zmq.PUB,
            encryption,
            self._log)

        self._queue = MessageQueueOutbound(self._socket, self._log)

    cpdef void start(self) except *:
        """
        Stop the server.
        """
        self._socket.connect()

    cpdef void stop(self) except *:
        """
        Stop the server.
        """
        self._socket.disconnect()

    cpdef void dispose(self) except *:
        """
        Dispose of the servers socket (call disconnect first).
        """
        self._socket.dispose()
        self._log.debug(f"Disposed.")

    cpdef void publish(self, str topic, bytes message) except *:
        """
        Publish the message to subscribers.

        :param topic: The topic of the message being published.
        :param message: The message bytes to send.
        """
        Condition.valid_string(topic, 'topic')

        cdef bytes body = self._compressor.compress(message)

        self._log.verbose(f"[{self.sent_count}]--> topic={topic}, body={len(body)} bytes")

        self._queue.send([topic.encode(UTF8), body])
        self.sent_count += 1
