# -------------------------------------------------------------------------------------------------
# <copyright file="mocks.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import uuid
import threading

import zmq

from nautilus_trader.model.commands cimport (
    AccountInquiry,
    SubmitOrder,
    SubmitAtomicOrder,
    ModifyOrder,
    CancelOrder)
from nautilus_trader.common.execution cimport ExecutionEngine, ExecutionClient
from nautilus_trader.common.logger cimport Logger, LoggerAdapter
from nautilus_trader.core.types cimport GUID
from nautilus_trader.network.responses cimport MessageReceived
from nautilus_trader.serialization.base cimport CommandSerializer, ResponseSerializer
from test_kit.stubs import TestStubs


cdef class ObjectStorer:
    """"
    A test class which stores the given objects.
    """

    def __init__(self):
        """
        Initializes a new instance of the ObjectStorer class.
        """
        self._store = []

    cpdef list get_store(self):
        """"
        Return the list or stored objects.
        
        return: List[Object].
        """
        return self._store

    cpdef void store(self, object obj):
        """"
        Store the given object.
        """
        self.count += 1
        self._store.append(obj)

    cpdef void store_2(self, object obj1, object obj2):
        """"
        Store the given objects as a tuple.
        """
        self.store((obj1, obj2))


cdef class MockExecutionClient(ExecutionClient):
    """
    Provides a mock execution client for testing. The mock will store all
    received commands in a list.
    """
    cdef readonly list received_commands

    def __init__(self, ExecutionEngine exec_engine, Logger logger):
        """
        Initializes a new instance of the MockExecutionClient class.

        :param exec_engine: The execution engine for the component.
        :param logger: The logger for the component.
        """
        super().__init__(exec_engine, logger)
        self.received_commands = []

    cpdef void connect(self):
        pass

    cpdef void disconnect(self):
        pass

    cpdef void dispose(self):
        pass

    cpdef void account_inquiry(self, AccountInquiry command):
        self.received_commands.append(command)

    cpdef void submit_order(self, SubmitOrder command):
        self.received_commands.append(command)

    cpdef void submit_atomic_order(self, SubmitAtomicOrder command):
        self.received_commands.append(command)

    cpdef void modify_order(self, ModifyOrder command):
        self.received_commands.append(command)

    cpdef void cancel_order(self, CancelOrder command):
        self.received_commands.append(command)

    cpdef void reset(self):
        self.received_commands = []


cdef class MockServer:
    """
    Provides a mock server.
    """
    cdef LoggerAdapter _log
    cdef str _service_address
    cdef object _zmq_context
    cdef object _socket
    cdef object _thread
    cdef int _cycles
    cdef list _responses

    def __init__(
            self,
            zmq_context: zmq.Context,
            int port,
            Logger logger,
            list responses=None):
        """
        Initializes a new instance of the MockServer class.

        :param zmq_context: The ZeroMQ context.
        :param port: The service port.
        :param logger: The logger for the component.
        :param responses: The responses to send.
        """
        if responses is None:
            responses = []
        super().__init__()
        self._log = LoggerAdapter(self.__class__.__name__, logger)
        self._service_address = f'tcp://127.0.0.1:{port}'
        self._zmq_context = zmq_context
        self._socket = self._zmq_context.socket(zmq.REP)
        self._socket.bind(self._service_address)
        self._thread = threading.Thread(target=self._consume_messages, daemon=True)
        self._cycles = 0
        self._responses = responses

        self._thread.start()

    def stop(self):
        """
        Close the connection and stop the mock server.
        """
        self._log.info(f"Unbinding from {self._service_address}...")
        self._socket.unbind(self._service_address)

    def _consume_messages(self):
        """
        Start the consumption loop to receive published messages.
        """
        self._log.info("Starting message consumption loop...")

        cdef bytes response
        if len(self._responses) > self._cycles:
            response = self._responses[self._cycles]
        else:
            response = "OK".encode('utf-8')

        while True:
            message = self._socket.recv()
            self._cycles += 1
            self._log.debug(f"Received[{self._cycles}] {message}")
            self._socket.send(response)


cdef class MockPublisher:
    """
    Provides a mock publisher.
    """
    cdef LoggerAdapter _log
    cdef str _service_address
    cdef object _zmq_context
    cdef object _socket
    cdef int _cycles

    def __init__(self,
                 zmq_context: zmq.Context,
                 int port,
                 Logger logger):
        """
        Initializes a new instance of the MockPublisher class.

        :param zmq_context: The ZeroMQ context.
        :param port: The service port.
        :param logger: The logger for the component.
        """
        self._log = LoggerAdapter(self.__class__.__name__, logger)
        self._service_address = f'tcp://127.0.0.1:{port}'
        self._zmq_context = zmq_context
        self._socket = self._zmq_context.socket(zmq.PUB)
        self._socket.bind(self._service_address)
        self._cycles = 0

        self._log.info(f"Bound to {self._service_address}...")

    def publish(self, str topic, bytes message):
        """
        Publish the message to the subscribers.

        :param topic: The topic of the message being published.
        :param message: The message bytes to send.
        """
        self._socket.send_multipart([topic.encode('utf-8'), message])
        self._cycles += 1
        self._log.debug(f"Publishing[{self._cycles}] topic={topic}, message={message}")

    def stop(self):
        """
        Stop the mock which unbinds then closes socket.
        """
        self._log.info(f"Unbinding from {self._service_address}...")
        self._socket.unbind(self._service_address)


cdef class MockCommandRouter:
    """
    Provides a mock command router.
    """
    cdef LoggerAdapter _log
    cdef str _service_address
    cdef CommandSerializer _command_serializer
    cdef ResponseSerializer _response_serializer
    cdef object _zmq_context
    cdef object _socket
    cdef object _thread
    cdef int _cycles

    cdef readonly list commands_received
    cdef readonly list responses_sent

    def __init__(
            self,
            zmq_context: zmq.Context,
            int port,
            CommandSerializer command_serializer,
            ResponseSerializer response_serializer,
            Logger logger):
        """
        Initializes a new instance of the MockCommandRouter class.

        :param zmq_context: The ZeroMQ context.
        :param port: The service port.
        :param command_serializer: The command serializer.
        :param response_serializer: The response serializer.
        :param logger: The logger for the component.
        """
        self._log = LoggerAdapter(self.__class__.__name__, logger)
        self._service_address = f'tcp://127.0.0.1:{port}'
        self._command_serializer = command_serializer
        self._response_serializer = response_serializer
        self._zmq_context = zmq_context
        self._socket = self._zmq_context.socket(zmq.REP)
        self._socket.bind(self._service_address)
        self._log.info(f"Bound to {self._service_address}...")
        self._cycles = 0

        self.commands_received = []
        self.responses_sent = []
        self._thread = threading.Thread(target=self._consume_messages, daemon=True)
        self._thread.start()

    def _consume_messages(self):
        """
        Start the consumption loop to receive published messages.
        """
        self._log.info("Running...")

        while True:
            message = self._command_serializer.deserialize(self._socket.recv())
            self.commands_received.append(message)
            self._cycles += 1
            self._log.debug(f"Received[{self._cycles}] {message}")

            response = MessageReceived(
                str(message),
                message.id,
                GUID(uuid.uuid4()),
                TestStubs.unix_epoch())

            self.responses_sent.append(response)
            self._socket.send(self._response_serializer.serialize(response))

    def stop(self):
        """
        Stop the router which unbinds then closes the socket.
        """
        self._log.debug(f"Unbinding from {self._service_address}...")
        self._socket.unbind(self._service_address)
