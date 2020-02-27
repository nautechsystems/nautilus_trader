# -------------------------------------------------------------------------------------------------
# <copyright file="node_servers.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import threading
import zmq
import zmq.auth

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Message, MessageType, message_type_to_string, message_type_from_string
from nautilus_trader.core.types cimport GUID
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.guid cimport GuidFactory
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.network.compression cimport Compressor
from nautilus_trader.network.encryption cimport EncryptionConfig
from nautilus_trader.network.identifiers cimport ClientId, ServerId, SessionId
from nautilus_trader.network.messages cimport Request, Response, MessageReceived, MessageRejected
from nautilus_trader.network.messages cimport Connect, Connected, Disconnect, Disconnected

cdef str _UTF8 = 'utf-8'


cdef class ServerNode(NetworkNode):
    """
    The base class for all client nodes.
    """

    def __init__(
            self,
            ServerId server_id not None,
            int port,
            int expected_frames,
            zmq_context not None: zmq.Context,
            int zmq_socket_type,
            Compressor compressor not None,
            EncryptionConfig encryption not None,
            Clock clock not None,
            GuidFactory guid_factory not None,
            Logger logger not None):
        """
        Initializes a new instance of the MQWorker class.

        :param server_id: The server identifier.
        :param port: The server port.
        :param zmq_context: The ZeroMQ context.
        :param zmq_socket_type: The ZeroMQ socket type.
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
        Condition.valid_port(port, 'port')
        Condition.type(zmq_context, zmq.Context, 'zmq_context')
        super().__init__(
            '127.0.0.1',
            port,
            expected_frames,
            zmq_context,
            zmq_socket_type,
            compressor,
            encryption,
            clock,
            guid_factory,
            logger)

        self.server_id = server_id
        self._socket.setsockopt(zmq.IDENTITY, self.server_id.value.encode(_UTF8))  # noqa (zmq reference)

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
            int expected_frames,
            zmq_context: zmq.Context,
            RequestSerializer request_serializer not None,
            ResponseSerializer response_serializer not None,
            Compressor compressor not None,
            EncryptionConfig encryption not None,
            Clock clock not None,
            GuidFactory guid_factory not None,
            Logger logger not None):
        """
        Initializes a new instance of the MessageServer class.

        :param zmq_context: The ZeroMQ context.
        :param port: The service port.
        :param request_serializer: The request serializer.
        :param response_serializer: The response serializer.
        :param logger: The logger for the component.
        """
        Condition.valid_port(port, 'port')
        Condition.type(zmq_context, zmq.Context, 'zmq_context')
        super().__init__(
            server_id,
            port,
            expected_frames,
            zmq_context,
            zmq.ROUTER,  # noqa (zmq reference)
            compressor,
            encryption,
            clock,
            guid_factory,
            logger)

        self._request_serializer = request_serializer
        self._response_serializer = response_serializer
        self._peers = {}    # type: {ClientId, SessionId}
        self._handlers = {} # type: {MessageType, callable}

        self._thread = threading.Thread(target=self._consume_messages, daemon=True)
        self._thread.start()

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
        Send a MessageReceived response for the given original message.
        
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
        Send the given response to the given client.
        
        Parameters
        ----------
        response : Response
            The response to send.
        receiver : ClientId
            The response receiver.
        """
        cdef bytes serialized = self._response_serializer.serialize(response)

        cdef bytes send_address = receiver.value.encode(_UTF8)
        cdef bytes header_type = message_type_to_string(response.message_type).encode(_UTF8)
        cdef bytes header_size = str(len(serialized)).encode(_UTF8)
        cdef bytes payload = self._compressor.compress(serialized)

        self._send([send_address, header_type, header_size, payload])

    cpdef void _consume_messages(self) except *:
        self._log.debug("Message consumption loop starting...")

        while True:
            try:
                self._handle_frames(self._socket.recv_multipart(flags=0))  # Blocking
            except zmq.ZMQError as ex:
                self._log.error(str(ex))
                continue
            self.sent_count += 1

    cpdef void _handle_frames(self, list frames) except *:
        cdef int frames_count = len(frames)
        if frames_count <= 0:
            self._log.error(f'Received zero frames with no reply address.')
            return

        cdef ClientId client_id = ClientId(frames[0].decode(_UTF8))
        if frames_count != self._expected_frames:
            message = f"Received unexpected frames count {frames_count}, expected {self._expected_frames}."
            self.send_rejected(message, GUID.none(), client_id)
            return

        cdef str header_type = frames[1].decode(_UTF8)
        cdef int recv_size = int(frames[2].decode(_UTF8))
        cdef bytes payload = self._compressor.decompress(frames[3])

        self._log.verbose(f"[{self.recv_count}]<-- "
                          f"header_type={header_type}, "
                          f"header_size={recv_size} bytes, "
                          f"payload={len(payload)} bytes")

        cdef MessageType message_type = message_type_from_string(header_type)
        if message_type == MessageType.REQUEST:
            self._handle_request(payload, client_id)
        else:
            handler = self._handlers.get(message_type)
            if handler is not None:
                handler(payload)

    cdef void _handle_request(self, bytes payload, ClientId sender) except *:
        cdef Request request = self._request_serializer.deserialize(payload)

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
            session_id = SessionId.create(client_id, self._clock.time_now())
            self._peers[client_id] = session_id
            message = f"{client_id.value} connected to session {session_id.value} at {self._network_address}"
            self._log.info(message)
        else:
            # Peer already connected to a session
            message = f"{client_id.value} already connected to session {session_id.value} at {self._network_address}"
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
            message = f"{client_id.value} had no session to disconnect at {self._network_address}"
            self._log.warning(message)
        else:
            # Peer connected to session
            del self._peers[client_id]
            message = f"{client_id.value} disconnected from {session_id} at {self._network_address}"
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
                 int expected_frames,
                 zmq_context: zmq.Context,
                 Compressor compressor not None,
                 EncryptionConfig encryption not None,
                 Clock clock not None,
                 GuidFactory guid_factory not None,
                 Logger logger not None):
        """
        Initializes a new instance of the MockPublisher class.

        :param zmq_context: The ZeroMQ context.
        :param port: The service port.
        :param logger: The logger for the component.
        """
        super().__init__(
            server_id,
            '127.0.0.1',
            port,
            expected_frames,
            zmq_context,
            zmq.PUB,
            compressor,
            encryption,
            clock,
            guid_factory,
            logger)

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
        self._send([topic.encode('utf-8'), bytes([len(message)]), message])
        self._log.debug(f"[{self.sent_count}]--> topic={topic}, message={message}")
