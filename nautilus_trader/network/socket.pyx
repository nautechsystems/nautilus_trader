# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import os
import zmq
import zmq.auth

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.types cimport Identifier
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.network.identifiers cimport ClientId, ServerId
from nautilus_trader.network.encryption cimport EncryptionSettings

cdef str _UTF8 = 'utf-8'


cdef class Socket:
    """
    The base class for all network sockets.
    """

    def __init__(
            self,
            Identifier socket_id not None,
            str host not None,
            int port,
            int socket_type,
            EncryptionSettings encryption not None,
            LoggerAdapter logger not None):
        """
        Initializes a new instance of the Socket class.

        :param socket_id: The socket identifier.
        :param host: The socket host address.
        :param port: The socket port.
        :param socket_type: The ZeroMQ socket type.
        :param encryption: The encryption configuration.
        :param logger: The logger for the component.
        :raises ValueError: If the host is not a valid string.
        :raises ValueError: If the port is not in range [49152, 65535].
        """
        Condition.valid_string(host, 'host')
        Condition.valid_port(port, 'port')

        self._log = logger
        self._socket = zmq.Context.instance().socket(socket_type)
        self._socket.setsockopt(zmq.IDENTITY, socket_id.value.encode(_UTF8))  # noqa (zmq reference)
        self._socket.setsockopt(zmq.LINGER, 1)

        self.socket_id = socket_id
        self.network_address = f'tcp://{host}:{port}'

        if encryption.use_encryption:
            if encryption.algorithm != 'curve':
                raise ValueError(f'Invalid encryption specified, was \'{encryption.algorithm}\'')
            key_file_client = os.path.join(encryption.keys_dir, "client.key_secret")
            key_file_server = os.path.join(encryption.keys_dir, "server.key")
            client_public, client_secret = zmq.auth.load_certificate(key_file_client)
            server_public, server_secret = zmq.auth.load_certificate(key_file_server)
            self._socket.curve_secretkey = client_secret
            self._socket.curve_publickey = client_public
            self._socket.curve_serverkey = server_public
            self._log.info(f"Curve25519 encryption setup for socket at {self.network_address}")
        else:
            self._log.warning(f"No encryption setup for socket at {self.network_address}")

    cpdef void connect(self) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void disconnect(self) except *:
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")

    cpdef void dispose(self) except *:
        """
        Dispose of the socket (call disconnect first).
        """
        self._socket.close()

    cpdef bint is_disposed(self):
        """
        Return a value indicating whether the internal socket is disposed.
    
        Returns
        -------
        bool
            True if the socket is disposed, else False.
        """
        return self._socket.closed

    cpdef void send(self, list frames) except *:
        """
        Send the given payload on the socket.
        """
        Condition.not_none(frames, 'frames')

        try:
            self._socket.send_multipart(frames)
        except zmq.ZMQError as ex:
            self._log.exception(ex)

    cpdef list recv(self):
        """
        Receive the next payload of frames from the socket.
        
        Returns
        -------
        list
            The list of bytes frames.
        """
        try:
            return self._socket.recv_multipart()
        except zmq.ZMQError as ex:
            self._log.exception(ex)
            return None


cdef class ClientSocket(Socket):
    """
    Provides a client socket.
    """

    def __init__(
            self,
            ClientId client_id not None,
            str host not None,
            int port,
            int socket_type,
            EncryptionSettings encryption not None,
            LoggerAdapter logger not None):
        """
        Initializes a new instance of the ClientSocket class.

        :param client_id: The client identifier.
        :param host: The socket host address.
        :param port: The socket port.
        :param socket_type: The ZeroMQ socket type.
        :param encryption: The encryption configuration.
        :param logger: The logger for the component.
        :raises ValueError: If the host is not a valid string.
        :raises ValueError: If the port is not in range [49152, 65535].
        """
        Condition.valid_string(host, 'host')
        Condition.valid_port(port, 'port')
        super().__init__(
            client_id,
            host,
            port,
            socket_type,
            encryption,
            logger)

    cpdef void connect(self) except *:
        """
        Connect the socket.
        """
        self._log.info(f"Connecting to {self.network_address}...")
        self._socket.connect(self.network_address)

    cpdef void disconnect(self) except *:
        """
        Disconnect the socket.
        """
        try:
            self._socket.disconnect(self.network_address)
        except zmq.ZMQError as ex:
            self._log.warning(f"Socket was not already connected to {self.network_address}")

        self._log.info(f"Disconnected from {self.network_address}")


cdef class SubscriberSocket(ClientSocket):
    """
    Provides a client socket.
    """

    def __init__(
            self,
            ClientId client_id not None,
            str host not None,
            int port,
            EncryptionSettings encryption not None,
            LoggerAdapter logger not None):
        """
        Initializes a new instance of the SubscriberSocket class.

        :param client_id: The client identifier.
        :param host: The socket host address.
        :param port: The socket port.
        :param encryption: The encryption configuration.
        :param logger: The logger for the component.
        :raises ValueError: If the host is not a valid string.
        :raises ValueError: If the port is not in range [49152, 65535].
        """
        Condition.valid_string(host, 'host')
        Condition.valid_port(port, 'port')
        super().__init__(
            client_id,
            host,
            port,
            zmq.SUB,
            encryption,
            logger)

    cpdef void connect(self) except *:
        """
        Connect the socket.
        """
        self._log.info(f"Connecting to {self.network_address}...")
        self._socket.connect(self.network_address)

    cpdef void disconnect(self) except *:
        """
        Disconnect the socket.
        """
        self._socket.disconnect(self.network_address)
        self._log.info(f"Disconnected from {self.network_address}")

    cpdef void subscribe(self, str topic) except *:
        """
        Subscribe the socket to the given topic.
        
        :param topic: The topic to subscribe to.
        """
        Condition.valid_string(topic, 'topic')

        self._socket.setsockopt(zmq.SUBSCRIBE, topic.encode(_UTF8))
        self._log.debug(f"Subscribed to topic {topic}")

    cpdef void unsubscribe(self, str topic) except *:
        """
        Unsubscribe the socket from the given topic.
        
        :param topic: The topic to unsubscribe from.
        """
        Condition.valid_string(topic, 'topic')

        self._socket.setsockopt(zmq.UNSUBSCRIBE, topic.encode(_UTF8))
        self._log.debug(f"Unsubscribed from topic {topic}")


cdef class ServerSocket(Socket):
    """
    Provides a server socket.
    """

    def __init__(
            self,
            ServerId server_id not None,
            int port,
            int socket_type,
            EncryptionSettings encryption not None,
            LoggerAdapter logger not None):
        """
        Initializes a new instance of the ServerSocket class.

        :param server_id: The server identifier.
        :param port: The socket port.
        :param socket_type: The ZeroMQ socket type.
        :param encryption: The encryption configuration.
        :param logger: The logger for the component.
        :raises ValueError: If the host is not a valid string.
        :raises ValueError: If the port is not in range [49152, 65535].
        """
        Condition.valid_port(port, 'port')
        super().__init__(
            server_id,
            '127.0.0.1',
            port,
            socket_type,
            encryption,
            logger)

    cpdef void connect(self) except *:
        """
        Connect the socket.
        """
        self._socket.bind(self.network_address)
        self._log.info(f"Bound socket to {self.network_address}")

    cpdef void disconnect(self) except *:
        """
        Disconnect the socket.
        """
        try:
            self._socket.unbind(self.network_address)
        except zmq.ZMQError as ex:
            self._log.warning(f"Socket was not already bound to {self.network_address}")

        self._log.info(f"Unbound socket at {self.network_address}")
