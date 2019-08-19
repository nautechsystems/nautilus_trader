# -------------------------------------------------------------------------------------------------
# <copyright file="dummies.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.model.commands cimport (
    AccountInquiry,
    SubmitOrder,
    SubmitAtomicOrder,
    ModifyOrder,
    CancelOrder)
from nautilus_trader.common.execution cimport ExecutionEngine, ExecutionClient
from nautilus_trader.common.logger cimport Logger


cdef class DummyExecutionClient(ExecutionClient):
    """
    Provides a dummy execution client which does nothing.
    """

    def __init__(self,
                 ExecutionEngine exec_engine,
                 Logger logger):
        """
        Initializes a new instance of the ExecutionClient class.

        :param exec_engine: The execution engine to connect to the client.
        :param logger: The logger for the component.
        """
        super().__init__(exec_engine, logger)

    cpdef void connect(self):
        pass

    cpdef void disconnect(self):
        pass

    cpdef void dispose(self):
        pass

    cpdef void account_inquiry(self, AccountInquiry command):
        pass

    cpdef void submit_order(self, SubmitOrder command):
        pass

    cpdef void submit_atomic_order(self, SubmitAtomicOrder command):
        pass

    cpdef void modify_order(self, ModifyOrder command):
        pass

    cpdef void cancel_order(self, CancelOrder command):
        pass

    cpdef void reset(self):
        pass
