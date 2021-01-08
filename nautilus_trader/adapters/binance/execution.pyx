# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import ccxt

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.live.execution cimport LiveExecutionClient
from nautilus_trader.execution.engine cimport ExecutionEngine
# from nautilus_trader.model.commands cimport CancelOrder
# from nautilus_trader.model.commands cimport ModifyOrder
# from nautilus_trader.model.commands cimport SubmitBracketOrder
# from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport Venue


cdef class BinanceExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for the `Binance` exchange.
    """

    def __init__(
        self,
        client not None: ccxt.Exchange,
        AccountId account_id not None,
        ExecutionEngine engine not None,
        Clock clock not None,
        Logger logger not None,
        dict config=None,
    ):
        """
        Initialize a new instance of the `ExecutionClient` class.

        Parameters
        ----------
        client : ccxt.Exchange
            The trading venue identifier for the client.
        account_id : AccountId
            The account identifier for the client.
        engine : ExecutionEngine
            The execution engine to connect to the client.
        clock : Clock
            The clock for the component.
        logger : Logger
            The logger for the component.
        config : dict[str, object], optional
            The configuration options.

        """
        super().__init__(
            Venue("BINANCE"),
            account_id,
            engine,
            clock,
            logger,
            config,
        )

        self._client = client
        self.initialized = True

        self._log.info(f"Initialized.")

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self.venue})"

    # cpdef bint is_connected(self) except *:
    #     """Abstract method (implement in subclass)."""
    #     raise NotImplementedError("method must be implemented in the subclass")
    #
    # cpdef void connect(self) except *:
    #     """Abstract method (implement in subclass)."""
    #     raise NotImplementedError("method must be implemented in the subclass")
    #
    # cpdef void disconnect(self) except *:
    #     """Abstract method (implement in subclass)."""
    #     raise NotImplementedError("method must be implemented in the subclass")
    #
    # cpdef void reset(self) except *:
    #     """Abstract method (implement in subclass)."""
    #     raise NotImplementedError("method must be implemented in the subclass")
    #
    # cpdef void dispose(self) except *:
    #     """Abstract method (implement in subclass)."""
    #     raise NotImplementedError("method must be implemented in the subclass")
    #
    # # -- COMMAND HANDLERS ------------------------------------------------------------------------------
    #
    # cpdef void submit_order(self, SubmitOrder command) except *:
    #     """Abstract method (implement in subclass)."""
    #     raise NotImplementedError("method must be implemented in the subclass")
    #
    # cpdef void submit_bracket_order(self, SubmitBracketOrder command) except *:
    #     """Abstract method (implement in subclass)."""
    #     raise NotImplementedError("method must be implemented in the subclass")
    #
    # cpdef void modify_order(self, ModifyOrder command) except *:
    #     """Abstract method (implement in subclass)."""
    #     raise NotImplementedError("method must be implemented in the subclass")
    #
    # cpdef void cancel_order(self, CancelOrder command) except *:
    #     """Abstract method (implement in subclass)."""
    #     raise NotImplementedError("method must be implemented in the subclass")
