#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="messaging.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import abc
import zmq

from typing import Callable
from threading import Thread
from zmq import Context

from inv_trader.core.checks import typechecking

UTF8 = 'utf-8'


class MQWorker(Thread):
    """
    The abstract base class for all MQ workers.
    """

    __metaclass__ = abc.ABCMeta

    @typechecking
    def __init__(
            self,
            name: str,
            context: Context,
            socket_type: int,
            host: str,
            port: int,
            handler: Callable):
        """
        Initializes a new instance of the MQWorker class.

        :param name: The name of the worker.
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
        self._socket = self._context.socket(socket_type)
        self._cycles = 0

    def run(self):
        """
        Overrides the threads run method (call .start() to run in a separate thread).
        Starts the worker and opens a connection.
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
        self._log(f"Stopped.")

    @abc.abstractmethod
    def _open_connection(self):
        """
        Open a new connection to the service.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the subclass.")

    @abc.abstractmethod
    def _close_connection(self):
        """
        Close the connection with the service.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the subclass.")

    @typechecking
    def _log(self, message: str):
        """
        Log the given message (if no logger then prints).

        :param message: The message to log.
        """
        print(f"{self._name}: {message}")


class RequestWorker(MQWorker):
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
        super().__init__(
            name,
            context,
            zmq.REQ,
            host,
            port,
            handler)

    def _open_connection(self):
        """
        Open a new connection to the service.
        """
        self._log(f"Connecting to {self._service_address}...")
        self._socket.connect(self._service_address)

    def _close_connection(self):
        """
        Close the connection with the service.
        """
        self._log(f"Disconnecting from {self._service_address}...")
        self._socket.disconnect(self._service_address)


class SubscriberWorker(MQWorker):
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
        super().__init__(
            name,
            context,
            zmq.SUB,
            host,
            port,
            handler)
        self._topic = topic

    def run(self):
        """
        Overrides the threads run method (call .start() to run in a separate thread).
        Starts the worker and opens a connection.
        """
        self._open_connection()

    def _open_connection(self):
        """
        Open a new connection to the service.
        """
        self._log(f"Connecting to {self._service_address}...")
        self._socket.connect(self._service_address)
        self._socket.setsockopt(zmq.SUBSCRIBE, self._topic.encode(UTF8))
        self._consume_messages()
        self._log(f"Subscribed to {self._topic}.")

    def _consume_messages(self):
        """
        Start the consumption loop to receive published messages.
        """
        self._log("Ready to consume messages...")

        while True:
            message = self._socket.recv()

            # Split on first occurrence of empty byte delimiter
            topic, data = message.split(b' ', 1)
            self._handler(data)
            self._cycles += 1
            self._log(f"Received message[{self._cycles}] from {topic}")

    def _close_connection(self):
        """
        Close the connection with the service.
        """
        self._log(f"Disconnecting from {self._service_address}...")
        self._socket.disconnect(self._service_address)
