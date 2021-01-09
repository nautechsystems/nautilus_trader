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

from nautilus_trader.adapters.ccxt.providers cimport CCXTInstrumentProvider
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport Logger
# from nautilus_trader.model.commands cimport CancelOrder
# from nautilus_trader.model.commands cimport ModifyOrder
# from nautilus_trader.model.commands cimport SubmitBracketOrder
# from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.live.execution cimport LiveExecutionClient
from nautilus_trader.live.execution cimport LiveExecutionEngine


cdef class CCXTExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for the unified CCXT Pro API.
    """

    def __init__(
        self,
        client not None,
        AccountId account_id not None,
        LiveExecutionEngine engine not None,
        LiveClock clock not None,
        Logger logger not None,
    ):
        """
        Initialize a new instance of the `CCXTExecutionClient` class.

        Parameters
        ----------
        client : ccxtpro.Exchange
            The unified CCXT client.
        account_id : AccountId
            The account identifier for the client.
        engine : LiveDataEngine
            The data engine for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.

        """
        super().__init__(
            Venue(client.name.upper()),
            account_id,
            engine,
            clock,
            logger,
            config={
                "name": f"CCXTExecClient-{client.name.upper()}",
            }
        )

        self._client = client
        self._instrument_provider = CCXTInstrumentProvider(
            client=client,
            load_all=False,
        )
        self._is_connected = False

        self._watch_account_task = None

    cpdef bint is_connected(self) except *:
        """
        Return a value indicating whether the client is connected.

        Returns
        -------
        bool
            True if connected, else False.

        """
        return self._is_connected

    cpdef void connect(self) except *:
        """
        Connect the client.
        """
        self._log.info("Connecting...")

        self._is_connected = True

        self._log.info("Connected.")

    cpdef void disconnect(self) except *:
        self._log.info("Disconnecting...")

        self._is_connected = False

        self._log.info("Disconnected.")

    cpdef void reset(self) except *:
        """
        Reset the client.
        """
        if self._is_connected:
            self._log.error("Cannot reset a connected execution client.")
            return

        self._log.info("Resetting...")

        # TODO: Reset client
        self._instrument_provider = CCXTInstrumentProvider(
            client=self._client,
            load_all=False,
        )

        self._log.info("Reset.")

    cpdef void dispose(self) except *:
        """
        Dispose the client.
        """
        if self._is_connected:
            self._log.error("Cannot dispose a connected execution client.")
            return

        self._log.info("Disposing...")

        # Nothing to dispose yet

        self._log.info("Disposed.")

# -- COMMAND HANDLERS ------------------------------------------------------------------------------

    # cpdef void submit_order(self, SubmitOrder command) except *:
    #     """Abstract method (implement in subclass)."""
    #     # TODO: Implement
    #
    # cpdef void submit_bracket_order(self, SubmitBracketOrder command) except *:
    #     """Abstract method (implement in subclass)."""
    #     # TODO: Implement
    #
    # cpdef void modify_order(self, ModifyOrder command) except *:
    #     """Abstract method (implement in subclass)."""
    #     # TODO: Implement
    #
    # cpdef void cancel_order(self, CancelOrder command) except *:
    #     """Abstract method (implement in subclass)."""
    #     # TODO: Implement
