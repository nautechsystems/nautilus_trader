# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from nautilus_trader.common.execution import ExecutionClient


class ObjectStorer:
    """"
    A test class which stores the given objects.
    """

    def __init__(self):
        """
        Initializes a new instance of the ObjectStorer class.
        """
        self.count = 0
        self._store = []

    def get_store(self) -> list:
        """"
        Return the list or stored objects.
        
        return: List[Object].
        """
        return self._store

    def store(self, obj):
        """"
        Store the given object.
        """
        self.count += 1
        self._store.append(obj)

    def store_2(self, obj1, obj2):
        """"
        Store the given objects as a tuple.
        """
        self.store((obj1, obj2))


class MockExecutionClient(ExecutionClient):
    """
    Provides an execution client for testing. The client will store all
    received commands in a list.
    """

    def __init__(self, exec_engine, logger):
        """
        Initializes a new instance of the MockExecutionClient class.

        :param exec_engine: The execution engine for the component.
        :param logger: The logger for the component.
        """
        super().__init__(exec_engine, logger)

        self.received_commands = []

    def connect(self):
        pass

    def disconnect(self):
        pass

    def dispose(self):
        pass

    def account_inquiry(self, command):
        self.received_commands.append(command)

    def submit_order(self, command):
        self.received_commands.append(command)

    def submit_atomic_order(self, command):
        self.received_commands.append(command)

    def modify_order(self, command):
        self.received_commands.append(command)

    def cancel_order(self, command):
        self.received_commands.append(command)

    def reset(self):
        self.received_commands = []
