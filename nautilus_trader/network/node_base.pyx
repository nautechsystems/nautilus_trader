# -------------------------------------------------------------------------------------------------
# <copyright file="node_base.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import os
import zmq
import zmq.auth

from nautilus_trader.core.message cimport MessageType, message_type_to_string
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.guid cimport GuidFactory
from nautilus_trader.common.logging cimport Logger, LoggerAdapter
from nautilus_trader.network.compression cimport Compressor
from nautilus_trader.network.encryption cimport EncryptionConfig

cdef str _UTF8 = 'utf-8'
cdef bytes _STRING = message_type_to_string(MessageType.STRING).encode(_UTF8)


cdef class NetworkNode:
    """
    The base class for all network nodes.
    """

    def __init__(
            self,
            str host,
            int port,
            int expected_frames,
            context not None: zmq.Context,
            int socket_type,
            Compressor compressor not None,
            EncryptionConfig encryption not None,
            Clock clock not None,
            GuidFactory guid_factory not None,
            Logger logger not None):
        """
        Initializes a new instance of the MQWorker class.

        :param host: The socket host address.
        :param port: The socket port.
        :param context: The ZeroMQ context.
        :param socket_type: The ZeroMQ socket type.
        :param expected_frames: The expected message frame count.
        :param compressor: The message compressor.
        :param encryption: The encryption configuration.
        :param clock: The clock for the component.
        :param guid_factory: The guid factory for the component.
        :param logger: The logger for the component.
        :raises ValueError: If the expected frames is not positive (> 0).
        :raises ValueError: If the host is not a valid string.
        :raises ValueError: If the port is not in range [49152, 65535].
        """
        Condition.positive(expected_frames, 'expected_frames')
        Condition.valid_string(host, 'host')
        Condition.valid_port(port, 'port')
        Condition.type(context, zmq.Context, 'zmq_context')
        super().__init__()

        self._clock = clock
        self._guid_factory = guid_factory
        self._log = LoggerAdapter(self.__class__.__name__, logger)
        self._network_address = f'tcp://{host}:{port}'
        self._context = context
        self._socket = self._context.socket(socket_type)
        self._socket.setsockopt(zmq.LINGER, 1)
        self._expected_frames = expected_frames
        self._compressor = compressor

        self.sent_count = 0
        self.recv_count = 0

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
            self._log.info(f"Curve25519 encryption setup for socket at {self._network_address}")
        else:
            self._log.warning(f"No encryption setup for socket at {self._network_address}")

    cpdef void dispose(self) except *:
        """
        Dispose of the MQWorker which close the socket (call disconnect first).
        """
        self._socket.close()
        self._log.debug(f"Disposed.")

    cpdef bint is_disposed(self):
        """
        Return a value indicating whether the internal socket is disposed.
    
        :return bool.
        """
        return self._socket.closed

    cdef void _send_string(self, bytes receiver, str message):
        self._send([receiver, _STRING, str(len(message)).encode(_UTF8), message.encode(_UTF8)])

    cdef void _send(self, list frames) except *:
        self._socket.send_multipart(frames)
        self.sent_count += 1
