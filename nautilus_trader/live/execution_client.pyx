# -------------------------------------------------------------------------------------------------
# <copyright file="execution_client.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import zmq

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Response
from nautilus_trader.model.commands cimport Command, AccountInquiry
from nautilus_trader.model.commands cimport SubmitOrder, SubmitAtomicOrder, ModifyOrder, CancelOrder
from nautilus_trader.model.events cimport Event
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.guid cimport GuidFactory
from nautilus_trader.common.logger cimport Logger
from nautilus_trader.common.execution cimport ExecutionEngine, ExecutionClient
from nautilus_trader.network.identifiers cimport ClientId
from nautilus_trader.network.node_clients cimport MessageClient, MessageSubscriber
from nautilus_trader.network.compression cimport Compressor, CompressorBypass
from nautilus_trader.network.encryption cimport EncryptionConfig
from nautilus_trader.serialization.base cimport CommandSerializer, ResponseSerializer
from nautilus_trader.serialization.serializers cimport MsgPackCommandSerializer, MsgPackResponseSerializer
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.guid cimport LiveGuidFactory
from nautilus_trader.live.logger cimport LiveLogger
from nautilus_trader.serialization.serializers cimport EventSerializer, MsgPackEventSerializer

cdef str _UTF8 = 'utf-8'

cdef class LiveExecClient(ExecutionClient):
    """
    Provides an execution client for live trading utilizing a ZMQ transport
    to the execution service.
    """

    def __init__(
            self,
            ExecutionEngine exec_engine not None,
            zmq_context: zmq.Context,
            str host='localhost',
            int commands_port=55555,
            int events_port=55556,
            Compressor compressor not None=CompressorBypass(),
            EncryptionConfig encryption not None=EncryptionConfig(),
            CommandSerializer command_serializer not None=MsgPackCommandSerializer(),
            ResponseSerializer response_serializer not None=MsgPackResponseSerializer(),
            EventSerializer event_serializer not None=MsgPackEventSerializer(),
            Clock clock not None=LiveClock(),
            GuidFactory guid_factory not None=LiveGuidFactory(),
            Logger logger not None=LiveLogger()):
        """
        Initializes a new instance of the LiveExecClient class.

        :param exec_engine: The execution engine for the component.
        :param zmq_context: The ZMQ context.
        :param host: The execution service host IP address (default='localhost').
        :param commands_port: The execution service commands port (default=55555).
        :param events_port: The execution service events port (default=55556).
        :param encryption: The encryption configuration.
        :param command_serializer: The command serializer for the client.
        :param response_serializer: The response serializer for the client.
        :param event_serializer: The event serializer for the client.
        :param clock: The clock for the component.
        :param guid_factory: The guid factory for the component.
        :param logger: The logger for the component.
        :raises ValueError: If the service_name is not a valid string.
        :raises ValueError: If the host is not a valid string.
        :raises ValueError: If the events_topic is not a valid string.
        :raises ValueError: If the commands_port is not in range [0, 65535].
        :raises ValueError: If the events_port is not in range [0, 65535].
        """
        Condition.valid_string(host, 'host')
        Condition.in_range_int(commands_port, 0, 65535, 'commands_port')
        Condition.in_range_int(events_port, 0, 65535, 'events_port')

        super().__init__(exec_engine, logger)
        self._zmq_context = zmq_context

        self.trader_id = exec_engine.trader_id
        self.client_id = ClientId(self.trader_id.value)

        expected_frames = 4

        self._commands_client = MessageClient(
            self.client_id,
            host,
            commands_port,
            self._zmq_context,
            expected_frames,
            self._response_handler,
            compressor,
            encryption,
            clock,
            guid_factory,
            logger)

        self._events_subscriber = MessageSubscriber(
            self.client_id,
            host,
            events_port,
            self._zmq_context,
            expected_frames,
            self._event_handler,
            compressor,
            encryption,
            clock,
            guid_factory,
            logger)

        self._command_serializer = command_serializer
        self._response_serializer = response_serializer
        self._event_serializer = event_serializer

    cpdef void connect(self) except *:
        """
        Connect to the execution service.
        """
        self._events_worker.connect()
        self._commands_worker.connect()
        self._events_worker.subscribe('Events')

    cpdef void disconnect(self) except *:
        """
        Disconnect from the execution service.
        """
        self._events_worker.unsubscribe('Events')
        self._commands_worker.disconnect()
        self._events_worker.disconnect()

    cpdef void dispose(self) except *:
        """
        Disposes of the execution client.
        """
        self._commands_worker.dispose()
        self._events_worker.dispose()

    cpdef void reset(self) except *:
        """
        Reset the execution client.
        """
        self._reset()

    cpdef void account_inquiry(self, AccountInquiry command) except *:
        self._command_handler(command)

    cpdef void submit_order(self, SubmitOrder command) except *:
        self._command_handler(command)

    cpdef void submit_atomic_order(self, SubmitAtomicOrder command) except *:
        self._command_handler(command)

    cpdef void modify_order(self, ModifyOrder command) except *:
        self._command_handler(command)

    cpdef void cancel_order(self, CancelOrder command) except *:
        self._command_handler(command)

    cpdef void _command_handler(self, Command command) except *:
        cdef bytes payload = self._command_serializer.serialize(command)
        self._commands_worker.send(command.message_type, payload)
        self._log.debug(f"Sent command {command}")

    cpdef void _response_handler(self, Response response) except *:
        self._log.debug(f"Received response {response}")

    cpdef void _event_handler(self, str topic, bytes event_bytes) except *:
        cdef Event event = self._event_serializer.deserialize(event_bytes)
        self._log.debug(f"Received event {event}")
        self._exec_engine.handle_event(event)
