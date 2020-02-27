# -------------------------------------------------------------------------------------------------
# <copyright file="mocks.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.model.commands cimport AccountInquiry, SubmitOrder, SubmitAtomicOrder
from nautilus_trader.model.commands cimport ModifyOrder, CancelOrder
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.execution cimport ExecutionEngine, ExecutionClient


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
    Provides an execution client for testing. The client will store all
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
