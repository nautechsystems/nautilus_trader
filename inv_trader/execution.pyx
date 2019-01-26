#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="execution.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False

import zmq

from inv_trader.core.precondition cimport Precondition
from inv_trader.common.clock cimport Clock, LiveClock
from inv_trader.common.guid cimport GuidFactory, LiveGuidFactory
from inv_trader.common.logger cimport Logger
from inv_trader.common.execution cimport ExecutionClient
from inv_trader.commands cimport Command, OrderCommand, CollateralInquiry
from inv_trader.commands cimport SubmitOrder, CancelOrder, ModifyOrder
from inv_trader.model.order cimport Order
from inv_trader.model.objects cimport Price
from inv_trader.model.events cimport Event
from inv_trader.model.identifiers cimport GUID
from inv_trader.messaging import RequestWorker, SubscriberWorker
from inv_trader.common.serialization cimport CommandSerializer, EventSerializer
from inv_trader.serialization cimport MsgPackCommandSerializer
from inv_trader.serialization cimport MsgPackEventSerializer

cdef str UTF8 = 'utf-8'


cdef class LiveExecClient(ExecutionClient):
    """
    Provides a client for the execution service utilizing a ZMQ transport.
    """
    cdef CommandSerializer _command_serializer
    cdef EventSerializer _event_serializer
    cdef object _commands_worker
    cdef object _events_worker

    cdef readonly object zmq_context

    def __init__(
            self,
            str host='localhost',
            int commands_port=5555,
            int events_port=5556,
            CommandSerializer command_serializer=MsgPackCommandSerializer(),
            EventSerializer event_serializer=MsgPackEventSerializer(),
            Clock clock=LiveClock(),
            GuidFactory guid_factory=LiveGuidFactory(),
            Logger logger=None):
        """
        Initializes a new instance of the LiveExecClient class.

        :param host: The execution service host IP address (default=127.0.0.1).
        :param commands_port: The execution service commands port.
        :param events_port: The execution service events port.
        :param command_serializer: The command serializer for the client.
        :param event_serializer: The event serializer for the client.
        :param clock: The internal clock for the component.
        :param guid_factory: The internal GUID factory for the component.
        :param logger: The logger for the component (can be None).
        :raises ValueError: If the host is not a valid string.
        :raises ValueError: If the commands_port is not in range [0, 65535]
        :raises ValueError: If the events_port is not in range [0, 65535]
        """
        Precondition.valid_string(host, 'host')
        Precondition.in_range(commands_port, 'commands_port', 0, 65535)
        Precondition.in_range(events_port, 'events_port', 0, 65535)

        super().__init__(clock, guid_factory, logger)
        self._command_serializer = command_serializer
        self._event_serializer = event_serializer
        self.zmq_context = zmq.Context()

        self._commands_worker = RequestWorker(
            'ExecClient.CommandSender',
            self.zmq_context,
            host,
            commands_port,
            self._command_ack_handler,
            logger)

        self._events_worker = SubscriberWorker(
            "ExecClient.EventSubscriber",
            self.zmq_context,
            host,
            events_port,
            "nautilus_execution_events",
            self._event_handler,
            logger)

        self._log.info(f"ZMQ v{zmq.pyzmq_version()}.")

    cpdef void connect(self):
        """
        Connect to the execution service and send a collateral inquiry command.
        """
        self._events_worker.start()
        self._commands_worker.start()

    cpdef void disconnect(self):
        """
        Disconnect from the execution service.
        """
        self._commands_worker.stop()
        self._events_worker.stop()

    cpdef void collateral_inquiry(self):
        """
        Send a collateral inquiry command to the execution service.
        """
        cdef Command command = CollateralInquiry(self._guid_factory.generate(),
                                                 self._clock.time_now())
        cdef bytes message = self._command_serializer.serialize(command)

        self._commands_worker.send(message)
        self._log.debug(f"Sent {command}")

    cpdef void submit_order(self, Order order, GUID strategy_id):
        """
        Send a submit order command to the execution service with the given
        order and strategy_id.

        :param order: The order to submit.
        :param strategy_id: The strategy identifier to register the order with.
        """
        self._register_order(order, strategy_id)

        cdef OrderCommand command = SubmitOrder(
            order,
            self._guid_factory.generate(),
            self._clock.time_now())
        cdef bytes message = self._command_serializer.serialize(command)

        self._commands_worker.send(message)
        self._log.debug(f"Sent {command}")

    cpdef void cancel_order(self, Order order, str cancel_reason):
        """
        Send a cancel order command to the execution service with the given
        order and cancel_reason.

        :param order: The order identifier to cancel.
        :param cancel_reason: The reason for cancellation (will be logged).
        :raises ValueError: If the cancel_reason is not a valid string.
        """
        Precondition.valid_string(cancel_reason, 'cancel_reason')

        cdef OrderCommand command = CancelOrder(
            order,
            cancel_reason,
            self._guid_factory.generate(),
            self._clock.time_now())
        cdef bytes message = self._command_serializer.serialize(command)

        self._commands_worker.send(message)
        self._log.debug(f"Sent {command}")

    cpdef void modify_order(self, Order order, Price new_price):
        """
        Send a modify order command to the execution service with the given
        order and new_price.

        :param order: The order identifier to modify.
        :param new_price: The new modified price for the order.
        :raises ValueError: If the new_price is not positive (> 0).
        """
        cdef OrderCommand command = ModifyOrder(
            order,
            new_price,
            self._guid_factory.generate(),
            self._clock.time_now())
        cdef bytes message = self._command_serializer.serialize(command)

        self._commands_worker.send(message)
        self._log.debug(f"Sent {command}")

    cdef void _event_handler(self, bytes body):
        """"
        Handle the event message by parsing to an Event and sending
        to the registered strategy.

        :param body: The order event message body.
        """
        cdef Event event = self._event_serializer.deserialize(body)

        # If no registered strategies then print message to console.
        if len(self._registered_strategies) == 0:
            self._log.debug(f"Received {event}")

        self._on_event(event)

    cdef void _command_ack_handler(self, bytes body):
        """"
        Handle the command acknowledgement message.

        :param body: The order command acknowledgement message body.
        """
        cdef Command command = self._command_serializer.deserialize(body)
        self._log.debug(f"Received order command ack for command_id {command.id}.")
