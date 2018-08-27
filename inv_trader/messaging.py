#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="messaging.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import json
import zmq

from typing import Callable
from threading import Thread
from collections import namedtuple
from zmq import Context

from inv_trader.core.checks import typechecking


# Holder for AMQP exchange properties.
SocketProps = namedtuple('SocketProps', 'exchange_name, exchange_type, queue_name, routing_key')


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
        self._name = name
        self._context = context
        self._service_address = f'tcp://{host}:{port}'
        self._handler = handler
        self._socket = self._context.socket(0)

    def run(self):
        """
        Beings the message queue process by connecting to the execution service
        and establish the messaging channels and queues needed.
        """
        self._open_connection()

    def _open_connection(self):
        """
        Open a new connection with the AMQP broker.

        :return: The pika connection object.
        """
        self._log(f"Connecting to {self._service_address}...")
        self._socket.connect(self._service_address)

    def _close_connection(self):
        """
        Open a new connection with the AMQP broker.

        :return: The pika connection object.
        """
        self._log(f"Disconnecting from {self._service_address}...")
        self._socket.disconnect(self._service_address)

    def send(self, message: bytes):
        """
        TBA

        :param message:
        :return:
        """
        self._socket.send(message)
        self._log(f"Sending {message}")

        response = self._socket.recv_string()
        self._log(f"Received {response}")

    def stop(self):
        """
        TBA
        """
        self._close_connection()

    @typechecking
    def _log(self, message: str):
        """
        Log the given message (if no logger then prints).

        :param message: The message to log.
        """
        print(f"{self._name}: {message}")
