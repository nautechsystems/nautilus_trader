
# -------------------------------------------------------------------------------------------------
# <copyright file="mocks.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import zmq

from cpython.datetime cimport datetime

from threading import Thread
from zmq import Context
from uuid import uuid4

from nautilus_trader.network.responses cimport MessageReceived
from nautilus_trader.model.identifiers cimport GUID
from nautilus_trader.serialization.base cimport CommandSerializer, ResponseSerializer
from test_kit.stubs import TestStubs

cdef datetime UNIX_EPOCH = TestStubs.unix_epoch()
cdef str UTF8 = 'utf-8'


class MockServer(Thread):

    def __init__(
            self,
            zmq_context: Context,
            int port):
        """
        Initializes a new instance of the MockServer class.

        :param zmq_context: The ZeroMQ context.
        :param port: The service port.
        """
        super().__init__()
        self.daemon = True
        self._service_address = f'tcp://127.0.0.1:{port}'
        self._zmq_context = zmq_context
        self._socket = self._zmq_context.socket(zmq.REP)
        self._cycles = 0

    def run(self):
        """
        Overrides the threads run method (call .start() to run in a separate thread).
        Starts the worker and opens a connection.
        """
        self._open_connection()

    def send(self, bytes message):
        """
        Send the given message to the connected requesters.

        :param message: The message bytes to send.
        """
        self._socket.send(message)
        self._cycles += 1
        self._log(f"Sending message[{self._cycles}] {message}")

        response = self._socket.recv()
        self._log(f"Received {response}")

    def stop(self):
        """
        Close the connection and stop the mock server.
        """
        self._close_connection()

    def _open_connection(self):
        """
        Open a new connection to the service..
        """
        self._log(f"Connecting to {self._service_address}...")
        self._socket.bind(self._service_address)
        self._consume_messages()

    def _consume_messages(self):
        """
        Start the consumption loop to receive published messages.
        """
        self._log("Ready to consume...")

        while True:
            message = self._socket.recv()
            self._cycles += 1
            self._log(f"Received message[{self._cycles}] {message}")
            self._socket.send("OK".encode(UTF8))

    def _close_connection(self):
        """
        Close the connection with the service socket.
        """
        self._log(f"Disconnecting from {self._service_address}...")
        self._socket.unbind(self._service_address)

    def _log(self, message: str):
        """
        Log the given message (if no logger then prints).

        :param message: The message to log.
        """
        print(f"MockServer: {message}")


class MockPublisher(Thread):

    def __init__(self, zmq_context: Context, int port):
        """
        Initializes a new instance of the MockPublisher class.

        :param zmq_context: The ZeroMQ context.
        :param port: The service port.
        """
        super().__init__()
        self.daemon = True
        self._service_address = f'tcp://127.0.0.1:{port}'
        self._zmq_context = zmq_context
        self._socket = self._zmq_context.socket(zmq.PUB)
        self._cycles = 0

    def run(self):
        """
        Overrides the threads run method.
        Starts the mock server and opens a connection (use the start method).
        """
        self._open_connection()

    def publish(
            self,
            str topic,
            bytes message):
        """
        Publish the message to the subscribers.

        :param topic: The topic of the message being published.
        :param message: The message bytes to send.
        """
        self._socket.send(topic.encode(UTF8) + b' ' + message)
        self._cycles += 1
        self._log(f"Publishing message[{self._cycles}] {message} for topic {topic}")

    def stop(self):
        """
        Close the connection and stop the publisher.
        """
        self._close_connection()

    def _open_connection(self):
        """
        Open a new connection to the service.
        """
        self._log(f"Connecting to {self._service_address}...")
        self._socket.bind(self._service_address)

    def _close_connection(self):
        """
        Close the connection with the service.
        """
        self._log(f"Disconnecting from {self._service_address}...")
        self._socket.disconnect(self._service_address)

    def _log(self, str message):
        """
        Log the given message (if no logger then prints).

        :param message: The message to log.
        """
        print(f"MockServer: {message}")


class MockCommandRouter(Thread):

    def __init__(
            self,
            zmq_context: Context,
            int port,
            CommandSerializer command_serializer,
            ResponseSerializer response_serializer):
        """
        Initializes a new instance of the MockCommandRouter class.

        :param zmq_context: The ZeroMQ context.
        :param port: The service port.
        :param command_serializer: The command serializer.
        :param response_serializer: The response serializer.
        """
        super().__init__()
        self.daemon = True
        self._service_address = f'tcp://127.0.0.1:{port}'
        self._command_serializer = command_serializer
        self._response_serializer = response_serializer
        self._zmq_context = zmq_context
        self._socket = self._zmq_context.socket(zmq.REP)
        self._cycles = 0
        self.commands_received = []  # List[Command]
        self.responses_sent = []     # List[Response]

    def run(self):
        """
        Overrides the threads run method (call .start() to run in a separate thread).
        Starts the worker and opens a connection.
        """
        self._open_connection()

    def stop(self):
        """
        Close the connection and stop the mock server.
        """
        self._close_connection()

    def _open_connection(self):
        """
        Open a new connection to the service..
        """
        self._log(f"Connecting to {self._service_address}...")
        self._socket.bind(self._service_address)
        self._consume_messages()

    def _consume_messages(self):
        """
        Start the consumption loop to receive published messages.
        """
        self._log("Ready to consume...")

        while True:
            message = self._command_serializer.deserialize(self._socket.recv())
            self.commands_received.append(message)
            self._cycles += 1
            self._log(f"Received message[{self._cycles}] {message}")

            response = MessageReceived(str(message), message.id, GUID(uuid4()), UNIX_EPOCH)
            self.responses_sent.append(response)
            self._socket.send(self._response_serializer.serialize(response))

    def _close_connection(self):
        """
        Close the connection with the service socket.
        """
        self._log(f"Disconnecting from {self._service_address}...")
        self._socket.unbind(self._service_address)

    def _log(self, message: str):
        """
        Log the given message (if no logger then prints).

        :param message: The message to log.
        """
        print(f"MockServer: {message}")
