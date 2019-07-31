# -------------------------------------------------------------------------------------------------
# <copyright file="execution.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import zmq

from queue import Queue
from threading import Thread
from zmq import Context

from nautilus_trader.core.precondition cimport Precondition
from nautilus_trader.core.message cimport MessageType, Message, Command, Event, Response
from nautilus_trader.model.commands cimport (
    CollateralInquiry,
    SubmitOrder,
    SubmitAtomicOrder,
    CancelOrder,
    ModifyOrder)
from nautilus_trader.common.account cimport Account
from nautilus_trader.common.clock cimport Clock, LiveClock
from nautilus_trader.common.guid cimport GuidFactory, LiveGuidFactory
from nautilus_trader.common.logger cimport Logger, LiveLogger
from nautilus_trader.common.execution cimport ExecutionClient
from nautilus_trader.trade.portfolio cimport Portfolio
from nautilus_trader.network.workers import RequestWorker, SubscriberWorker
from nautilus_trader.serialization.base cimport CommandSerializer, ResponseSerializer, EventSerializer
from nautilus_trader.serialization.serializers cimport (
    MsgPackCommandSerializer,
    MsgPackResponseSerializer,
    MsgPackEventSerializer
)


cdef class LiveExecClient(ExecutionClient):
    """
    Provides an execution client for live trading utilizing a ZMQ transport
    to the execution service.
    """
    cdef object _message_bus
    cdef object _thread
    cdef object _commands_worker
    cdef object _events_worker
    cdef CommandSerializer _command_serializer
    cdef ResponseSerializer _response_serializer
    cdef EventSerializer _event_serializer

    cdef readonly object zmq_context

    def __init__(
            self,
            zmq_context: Context,
            str service_address='localhost',
            str events_topic='NAUTILUS:EXECUTION',
            int commands_port=55555,
            int events_port=55556,
            CommandSerializer command_serializer=MsgPackCommandSerializer(),
            ResponseSerializer response_serializer=MsgPackResponseSerializer(),
            EventSerializer event_serializer=MsgPackEventSerializer(),
            Account account=Account(),
            Portfolio portfolio=Portfolio(),
            Clock clock=LiveClock(),
            GuidFactory guid_factory=LiveGuidFactory(),
            Logger logger=LiveLogger()):
        """
        Initializes a new instance of the LiveExecClient class.

        :param zmq_context: The ZMQ context.
        :param service_address: The execution service host IP address (default='localhost').
        :param events_topic: The execution service events topic (default='NAUTILUS:EXECUTION').
        :param commands_port: The execution service commands port (default=55555).
        :param events_port: The execution service events port (default=55556).
        :param command_serializer: The command serializer for the client.
        :param response_serializer: The response serializer for the client.
        :param event_serializer: The event serializer for the client.
        :param account: The account for the execution client.
        :param clock: The clock for the component.
        :param guid_factory: The GUID factory for the component.
        :param logger: The logger for the component (can be None).
        :raises ValueError: If the service_address is not a valid string.
        :raises ValueError: If the events_topic is not a valid string.
        :raises ValueError: If the commands_port is not in range [0, 65535]
        :raises ValueError: If the events_port is not in range [0, 65535]
        """
        Precondition.valid_string(service_address, 'service_address')
        Precondition.valid_string(events_topic, 'events_topic')
        Precondition.in_range(commands_port, 'commands_port', 0, 65535)
        Precondition.in_range(events_port, 'events_port', 0, 65535)

        super().__init__(account,
                         portfolio,
                         clock,
                         guid_factory,
                         logger)
        self.zmq_context = zmq_context
        self._message_bus = Queue()
        self._thread = Thread(target=self._process, daemon=True)
        self._commands_worker = RequestWorker(
            'ExecClient.CommandSender',
            'NAUTILUS:COMMAND_ROUTER',
            service_address,
            commands_port,
            self.zmq_context,
            logger)
        self._events_worker = SubscriberWorker(
            'ExecClient.EventSubscriber',
            'NAUTILUS:EVENTS_PUBLISHER',
            service_address,
            events_port,
            self.zmq_context,
            self._deserialize_event,
            logger)
        self._events_worker.subscribe(events_topic)
        self._command_serializer = command_serializer
        self._response_serializer = response_serializer
        self._event_serializer = event_serializer

        self._log.info(f"ZMQ v{zmq.pyzmq_version()}.")
        self._thread.start()

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

    cpdef void reset(self):
        """
        Resets the live execution client by clearing all stateful internal values 
        and returning it to a fresh state.
        """
        self._reset()

    cpdef void dispose(self):
        """
        Disposes of the live execution client.
        """
        self.zmq_context.term()

    cpdef void execute_command(self, Command command):
        """
        Execute the given command by inserting it into the message bus for processing.
        
        :param command: The command to execute.
        """
        self._message_bus.put(command)

    cpdef void handle_event(self, Event event):
        """
        Handle the given event by inserting it into the message bus for processing.
        
        :param event: The event to handle
        """
        self._message_bus.put(event)

    cpdef void _process(self):
        cdef Message message
        while True:
            message = self._message_bus.get()
            if message.message_type == MessageType.EVENT:
                self._handle_event(message)
            elif message.message_type == MessageType.COMMAND:
                self._execute_command(message)
            else:
                raise RuntimeError(f"Invalid message type on bus ({type(message)}).")

    cpdef void _collateral_inquiry(self, CollateralInquiry command):
        self._send_command(command)

    cpdef void _submit_order(self, SubmitOrder command):
        self._send_command(command)

    cpdef void _submit_atomic_order(self, SubmitAtomicOrder command):
        self._send_command(command)

    cpdef void _modify_order(self, ModifyOrder command):
        self._send_command(command)

    cpdef void _cancel_order(self, CancelOrder command):
        self._send_command(command)

    cpdef void _send_command(self, Command command):
        self._log.debug(f"Sending {command}")
        cdef bytes response_bytes = self._commands_worker.send(self._command_serializer.serialize(command))
        cdef Response response =  self._deserialize_response(response_bytes)
        self._log.debug(f"Received response {response}")

    cpdef void _deserialize_event(self, str topic, bytes event_bytes):
        cdef Event event = self._event_serializer.deserialize(event_bytes)

        # If no registered strategies then just log
        if len(self._registered_strategies) == 0:
            self._log.info(f"Received {event} for topic {topic}.")

        self._handle_event(event)

    cpdef void _deserialize_response(self, bytes response_bytes):
        cdef Response response = self._response_serializer.deserialize(response_bytes)
        self._log.debug(f"Received {response}")
