#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="mocks.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False


import uuid
import zmq

from datetime import datetime
from threading import Thread
from typing import Callable
from zmq import Context

from inv_trader.core.decimal cimport Decimal
from inv_trader.common.execution cimport ExecutionClient
from inv_trader.model.objects import Price
from inv_trader.model.order cimport Order
from inv_trader.model.events cimport OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events cimport OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events cimport OrderFilled, OrderPartiallyFilled
from inv_trader.model.identifiers cimport GUID, OrderId, ExecutionId, ExecutionTicket

cdef str UTF8 = 'utf-8'


class MockServer(Thread):

    def __init__(
            self,
            context: Context,
            port: int,
            handler: Callable):
        """
        Initializes a new instance of the MockServer class.

        :param context: The ZeroMQ context.
        :param port: The service port.
        :param handler: The response handler.
        """
        super().__init__()
        self.daemon = True
        self._context = context
        self._service_address = f'tcp://127.0.0.1:{port}'
        self._handler = handler
        self._socket = self._context.socket(zmq.REP)
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
            self._handler(message)
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

    def __init__(
            self,
            context: Context,
            int port,
            handler: Callable):
        """
        Initializes a new instance of the MockServer class.

        :param context: The ZeroMQ context.
        :param port: The service port.
        :param handler: The response handler.
        """
        super().__init__()
        self.daemon = True
        self._context = context
        self._service_address = f'tcp://127.0.0.1:{port}'
        self._handler = handler
        self._socket = self._context.socket(zmq.PUB)
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


cdef class MockExecClient(ExecutionClient):
    """
    Provides a mock execution client for trading strategies.
    """
    cdef list _working_orders

    def __init__(self):
        """
        Initializes a new instance of the MockExecClient class.
        """
        super().__init__()
        self._working_orders = []

    def connect(self):
        """
        Connect to the execution service.
        """
        self._log.info("MockExecClient connected.")

    def disconnect(self):
        """
        Disconnect from the execution service.
        """
        self._log.info("MockExecClient disconnected.")

    cpdef void submit_order(self, Order order, GUID strategy_id):
        """
        Send a submit order command to the mock execution service.
        """
        self._register_order(order, strategy_id)

        cdef OrderSubmitted submitted = OrderSubmitted(
            order.symbol,
            order.id,
            datetime.utcnow(),
            GUID(uuid.uuid4()),
            datetime.utcnow())

        cdef OrderAccepted accepted = OrderAccepted(
            order.symbol,
            order.id,
            datetime.utcnow(),
            GUID(uuid.uuid4()),
            datetime.utcnow())

        self._working_orders.append(order)

        cdef OrderWorking working = OrderWorking(
            order.symbol,
            order.id,
            OrderId('B' + str(order.id)),
            order.label,
            order.side,
            order.type,
            order.quantity,
            Decimal('1'),
            order.time_in_force,
            datetime.utcnow(),
            GUID(uuid.uuid4()),
            datetime.utcnow(),
            order.expire_time)

        self._on_event(submitted)
        self._on_event(accepted)
        self._on_event(working)

    cpdef void cancel_order(self, Order order, str cancel_reason):
        """
        Send a cancel order command to the mock execution service.
        """
        cdef OrderCancelled cancelled = OrderCancelled(
            order.symbol,
            order.id,
            datetime.utcnow(),
            GUID(uuid.uuid4()),
            datetime.utcnow())

        self._on_event(cancelled)

    cpdef void modify_order(self, Order order, new_price: Decimal):
        """
        Send a modify order command to the mock execution service.
        """
        cdef OrderModified modified = OrderModified(
            order.symbol,
            order.id,
            OrderId('B' + str(order.id)),
            new_price,
            datetime.utcnow(),
            GUID(uuid.uuid4()),
            datetime.utcnow())

        self._on_event(modified)

    cpdef void collateral_inquiry(self):
        """
        Send a collateral inquiry command to the mock execution service.
        """
        # Does nothing.

    cpdef void fill_last_order(self):
        """
        Fills the last order held by the execution service.
        """
        order = self._working_orders.pop(-1)

        cdef Decimal filled_price = Price.create(1.00000, 5) if order.price is None else order.price

        cdef OrderFilled filled = OrderFilled(
            order.symbol,
            order.id,
            ExecutionId('E' + str(order.id)),
            ExecutionTicket('ET' + str(order.id)),
            order.side,
            order.quantity,
            filled_price,
            datetime.utcnow(),
            GUID(uuid.uuid4()),
            datetime.utcnow())

        self._on_event(filled)
