#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="mocks.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, nonecheck=False

import uuid
import zmq

from threading import Thread
from typing import Callable, List, Dict
from zmq import Context

from cpython.datetime cimport datetime

from inv_trader.common.execution cimport ExecutionClient
from inv_trader.common.clock cimport TestClock
from inv_trader.common.guid cimport TestGuidFactory
from inv_trader.common.logger cimport TestLogger
from inv_trader.common.account cimport Account
from inv_trader.model.objects cimport Price
from inv_trader.model.order cimport Order
from inv_trader.model.events cimport Event, OrderSubmitted, OrderAccepted, OrderWorking
from inv_trader.model.events cimport  OrderModified, OrderCancelled, OrderFilled
from inv_trader.model.identifiers cimport GUID, OrderId, ExecutionId, ExecutionTicket
from inv_trader.commands cimport Command, CollateralInquiry
from inv_trader.commands cimport SubmitOrder, SubmitAtomicOrder, ModifyOrder, CancelOrder
from inv_trader.portfolio.portfolio cimport Portfolio

cdef str UTF8 = 'utf-8'


class MockServer(Thread):

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
    cdef readonly list working_orders
    cdef readonly dict atomic_orders

    def __init__(self):
        """
        Initializes a new instance of the MockExecClient class.
        """
        cdef Portfolio portfolio = Portfolio()
        portfolio.register_execution_client(self)
        super().__init__(
            Account(),
            portfolio,
            TestClock(),
            TestGuidFactory(),
            TestLogger())

        self.working_orders = []  # type: List[Order]
        self.atomic_orders = {}   # type: Dict[OrderId, List[Order]]

    cpdef void connect(self):
        """
        Connect to the execution service.
        """
        self._log.info("MockExecClient connected.")

    cpdef void disconnect(self):
        """
        Disconnect from the execution service.
        """
        self._log.info("MockExecClient disconnected.")

    cpdef void process_queue(self):
        """
        Process the internal queue of commands and events.
        """
        while not self._queue.empty():
            item = self._queue.get()

            if isinstance(item, Event):
                self._handle_event(item)
            elif isinstance(item, Command):
                self._execute_command(item)

    cpdef void fill_last_order(self):
        """
        Fills the last working order.
        """
        cdef Order order = self.working_orders.pop(-1)

        cdef OrderFilled filled = OrderFilled(
            order.symbol,
            order.id,
            ExecutionId('E' + str(order.id)),
            ExecutionTicket('ET' + str(order.id)),
            order.side,
            order.quantity,
            Price('1.00000') if order.price is None else order.price,
            datetime.utcnow(),
            GUID(uuid.uuid4()),
            datetime.utcnow())

        self.handle_event(filled)

        if order.id in self.atomic_orders:
            for order in self.atomic_orders[order.id]:
                self._work_order(order)

    cdef void _work_order(self, Order order):
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

        self.working_orders.append(order)

        cdef OrderWorking working = OrderWorking(
            order.symbol,
            order.id,
            OrderId('B' + str(order.id)),
            order.label,
            order.side,
            order.type,
            order.quantity,
            Price('1.00000'),
            order.time_in_force,
            datetime.utcnow(),
            GUID(uuid.uuid4()),
            datetime.utcnow(),
            order.expire_time)

        self.handle_event(submitted)
        self.handle_event(accepted)
        self.handle_event(working)

    cdef void _submit_order(self, SubmitOrder command):
        """
        Send a submit order command to the mock execution service.
        """
        self._work_order(command.order)

    cdef void _submit_atomic_order(self, SubmitAtomicOrder command):
        """
        Send a submit atomic order command to the mock execution service.
        """
        cdef list atomic_orders = [command.atomic_order.stop_loss]
        if command.atomic_order.has_profit_target:
            atomic_orders.append(command.atomic_order.profit_target)

        self.atomic_orders[command.atomic_order.id] = atomic_orders
        self._work_order(command.atomic_order.entry)

    cdef void _modify_order(self, ModifyOrder command):
        """
        Send a modify order command to the mock execution service.
        """
        cdef OrderModified modified = OrderModified(
            command.order.symbol,
            command.order.id,
            OrderId('B' + str(command.order.id)),
            command.modified_price,
            datetime.utcnow(),
            GUID(uuid.uuid4()),
            datetime.utcnow())

        self.handle_event(modified)

    cdef void _cancel_order(self, CancelOrder command):
        """
        Send a cancel order command to the mock execution service.
        """
        cdef OrderCancelled cancelled = OrderCancelled(
            command.order.symbol,
            command.order.id,
            datetime.utcnow(),
            GUID(uuid.uuid4()),
            datetime.utcnow())

        self.handle_event(cancelled)

    cdef void _collateral_inquiry(self, CollateralInquiry command):
        """
        Send a collateral inquiry command to the mock execution service.
        """
        # Do nothing
