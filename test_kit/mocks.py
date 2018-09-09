#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="execution.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import uuid
import zmq

from datetime import datetime
from decimal import Decimal
from threading import Thread
from typing import Callable
from uuid import UUID
from zmq import Context

from inv_trader.model.enums import Venue, Resolution, QuoteType, OrderSide, OrderType, OrderStatus
from inv_trader.model.objects import Symbol, BarType, Bar
from inv_trader.execution import ExecutionClient
from inv_trader.model.objects import Price
from inv_trader.model.order import Order
from inv_trader.model.events import Event, OrderEvent
from inv_trader.model.events import OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events import OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events import OrderFilled, OrderPartiallyFilled

UTF8 = 'utf-8'
StrategyId = str
OrderId = str


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

    def send(self, message: bytes):
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
            topic: str,
            message: bytes):
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

    def _log(self, message: str):
        """
        Log the given message (if no logger then prints).

        :param message: The message to log.
        """
        print(f"MockServer: {message}")


class MockExecClient(ExecutionClient):
    """
    Provides a mock execution client for trading strategies.
    """

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

    def submit_order(
            self,
            order: Order,
            strategy_id: UUID):
        """
        Send a submit order command to the mock execution service.
        """
        super()._register_order(order, strategy_id)

        submitted = OrderSubmitted(
            order.symbol,
            order.id,
            datetime.utcnow(),
            uuid.uuid4(),
            datetime.utcnow())

        accepted = OrderAccepted(
            order.symbol,
            order.id,
            datetime.utcnow(),
            uuid.uuid4(),
            datetime.utcnow())

        self._working_orders.append(order)

        working = OrderWorking(
            order.symbol,
            order.id,
            'B' + order.id,
            order.label,
            order.side,
            order.type,
            order.quantity,
            Decimal('1'),
            order.time_in_force,
            datetime.utcnow(),
            uuid.uuid4(),
            datetime.utcnow(),
            order.expire_time)

        super()._on_event(submitted)
        super()._on_event(accepted)
        super()._on_event(working)

    def cancel_order(
            self,
            order: Order,
            cancel_reason: str):
        """
        Send a cancel order command to the mock execution service.
        """
        cancelled = OrderCancelled(
            order.symbol,
            order.id,
            datetime.utcnow(),
            uuid.uuid4(),
            datetime.utcnow())

        super()._on_event(cancelled)

    def modify_order(self, order: Order, new_price: Decimal):
        """
        Send a modify order command to the mock execution service.
        """
        modified = OrderModified(
            order.symbol,
            order.id,
            'B' + order.id,
            new_price,
            datetime.utcnow(),
            uuid.uuid4(),
            datetime.utcnow())

        super()._on_event(modified)

    def collateral_inquiry(self):
        """
        Send a collateral inquiry command to the mock execution service.
        """
        # Does nothing.

    def fill_last_order(self):
        """
        Fills the last order held by the execution service.
        """
        order = self._working_orders.pop(-1)

        filled_price = Price.create(1.00000, 5) if order.price is None else order.price

        filled = OrderFilled(
            order.symbol,
            order.id,
            'E' + order.id,
            'ET' + order.id,
            order.side,
            order.quantity,
            filled_price,
            datetime.utcnow(),
            uuid.uuid4(),
            datetime.utcnow())

        super()._on_event(filled)
