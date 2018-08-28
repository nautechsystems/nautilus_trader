#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="messaging.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import zmq

from typing import Callable
from threading import Thread
from zmq import Context

from inv_trader.core.checks import typechecking

UTF8 = 'utf-8'


class RequestWorker(Thread):
    @typechecking
    def __init__(
            self,
            name: str,
            context: Context,
            host: str,
            port: int,
            handler: Callable):
        """
        Initializes a new instance of the RequestWorker class.

        :param context: The ZeroMQ context.
        :param host: The service host address.
        :param port: The service port.
        :param handler: The response handler.
        """
        super().__init__()
        self.daemon = True
        self._name = name
        self._context = context
        self._service_address = f'tcp://{host}:{port}'
        self._handler = handler
        self._socket = self._context.socket(zmq.REQ)
        self._cycles = 0

    def run(self):
        """
        Overrides the threads run method.
        Starts the worker and opens a connection (use the start method).
        """
        self._open_connection()

    def send(self, message: bytes):
        """
        Send the message to the service socket.

        :param message: The message bytes to send.
        """
        self._socket.send(message)
        self._cycles += 1
        self._log(f"Sending message[{self._cycles}] {message}")

        response = self._socket.recv()
        self._log(f"Received {response.decode(UTF8)}[{self._cycles}] response.")

    def stop(self):
        """
        Close the connection and stop the worker.
        """
        self._close_connection()

    def _open_connection(self):
        """
        Open a new connection to the service socket.
        """
        self._log(f"Connecting to {self._service_address}...")
        self._socket.connect(self._service_address)

    def _close_connection(self):
        """
        Close the connection with the service socket.
        """
        self._log(f"Disconnecting from {self._service_address}...")
        self._socket.disconnect(self._service_address)

    @typechecking
    def _log(self, message: str):
        """
        Log the given message (if no logger then prints).

        :param message: The message to log.
        """
        print(f"{self._name}: {message}")


class SubscriberWorker(Thread):
    @typechecking
    def __init__(
            self,
            name: str,
            context: Context,
            host: str,
            port: int,
            topic: str,
            handler: Callable):
        """
        Initializes a new instance of the SubscriberWorker class.

        :param context: The ZeroMQ context.
        :param host: The service host address.
        :param port: The service port.
        :param topic: The topic to subscribe to.
        :param handler: The response handler.
        """
        super().__init__()
        self._name = name
        self._context = context
        self._service_address = f'tcp://{host}:{port}'
        self._handler = handler
        self._socket = self._context.socket(zmq.SUB)
        self._topic = topic
        self._cycles = 0

    def run(self):
        """
        Overrides the threads run method.
        Starts the worker and opens a connection (use the start method).
        """
        self._open_connection()

    def send(self, message: bytes):
        """
        Send the message to the service socket.

        :param message: The message bytes to send.
        """
        self._socket.send(message)
        self._log(f"Sending {message}")

        response = self._socket.recv()
        self._log(f"Received {response.decode(UTF8)}[{self._cycles}] response.")

    def stop(self):
        """
        Close the connection and stop the worker.
        """
        self._close_connection()

    def _open_connection(self):
        """
        Open a new connection to the service socket..
        """
        self._log(f"Connecting to {self._service_address}...")
        self._socket.connect(self._service_address)
        self._socket.setsockopt(zmq.SUBSCRIBE, self._topic)
        self._consume_messages()

    def _consume_messages(self):
        """
        Start the consumption loop to receive published messages.
        """
        while True:
            message = self._socket.recv()
            topic, data = message.split()
            self._handler(data)
            self._cycles += 1
            self._log(f"Received message[{self._cycles}] from {topic}")

    def _close_connection(self):
        """
        Close the connection with the service socket.
        """
        self._log(f"Disconnecting from {self._service_address}...")
        self._socket.disconnect(self._service_address)

    @typechecking
    def _log(self, message: str):
        """
        Log the given message (if no logger then prints).

        :param message: The message to log.
        """
        print(f"{self._name}: {message}")
