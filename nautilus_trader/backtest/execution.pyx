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

from nautilus_trader.backtest.exchange cimport SimulatedExchange
from nautilus_trader.common.clock cimport TestClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.execution.client cimport ExecutionClient
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.model.c_enums.account_type cimport AccountType
from nautilus_trader.model.commands.trading cimport CancelOrder
from nautilus_trader.model.commands.trading cimport SubmitBracketOrder
from nautilus_trader.model.commands.trading cimport SubmitOrder
from nautilus_trader.model.commands.trading cimport UpdateOrder
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientId


cdef class BacktestExecClient(ExecutionClient):
    """
    Provides an execution client for the `BacktestEngine`.
    """

    def __init__(
        self,
        SimulatedExchange exchange not None,
        AccountId account_id not None,
        AccountType account_type,
        Currency base_currency,  # Can be None
        ExecutionEngine engine not None,
        TestClock clock not None,
        Logger logger not None,
        bint is_frozen_account=False,
    ):
        """
        Initialize a new instance of the ``BacktestExecClient`` class.

        Parameters
        ----------
        exchange : SimulatedExchange
            The simulated exchange for the backtest.
        account_id : AccountId
            The account ID for the client.
        account_type : AccountType
            The account type for the client.
        base_currency : Currency, optional
            The account base currency for the client. Use ``None`` for multi-currency accounts.
        engine : ExecutionEngine
            The execution engine for the client.
        clock : TestClock
            The clock for the component.
        logger : Logger
            The logger for the component.
        is_frozen_account : bool
            If the backtest run account is frozen.

        """
        super().__init__(
            client_id=ClientId(exchange.id.value),
            venue_type=exchange.venue_type,
            account_id=account_id,
            account_type=account_type,
            base_currency=base_currency,
            engine=engine,
            clock=clock,
            logger=logger,
            config={"calculate_account_state": False if is_frozen_account else True},
        )

        self._exchange = exchange
        self.is_connected = False

    cpdef void connect(self) except *:
        """
        Connect the client.
        """
        self._log.info("Connecting...")

        self.is_connected = True
        self._log.info("Connected.")

    cpdef void disconnect(self) except *:
        """
        Disconnect the client.
        """
        self._log.info("Disconnecting...")

        self.is_connected = False
        self._log.info("Disconnected.")

    cpdef void reset(self) except *:
        """
        Reset the client.

        All stateful fields are reset to their initial value.
        """
        self._log.info(f"Resetting...")

        # Nothing to reset
        self._log.info("Reset.")

    cpdef void dispose(self) except *:
        """
        Dispose of the client.

        This method is idempotent and irreversible. No other methods should be
        called after disposal.
        """
        self._log.info(f"Disposing.")

        # Nothing to dispose
        self._log.info(f"Disposed.")

# -- COMMAND HANDLERS ------------------------------------------------------------------------------

    cpdef void submit_order(self, SubmitOrder command) except *:
        """
        Submit the order contained in the given command for execution.

        Parameters
        ----------
        command : SubmitOrder
            The command to execute.

        """
        if not self.is_connected:  # Simulate connection behaviour
            self._log.error(f"Cannot send command (not connected), {command}.")
            return

        self._exchange.handle_submit_order(command)

    cpdef void submit_bracket_order(self, SubmitBracketOrder command) except *:
        """
        Submit the bracket order contained in the given command for execution.

        Parameters
        ----------
        command : SubmitBracketOrder
            The command to execute.

        """
        if not self.is_connected:  # Simulate connection behaviour
            self._log.error(f"Cannot send command (not connected), {command}.")
            return

        self._exchange.handle_submit_bracket_order(command)

    cpdef void update_order(self, UpdateOrder command) except *:
        """
        Amend the order with parameters contained in the command.

        Parameters
        ----------
        command : UpdateOrder
            The command to execute.

        """
        if not self.is_connected:  # Simulate connection behaviour
            self._log.error(f"Cannot send command (not connected), {command}.")
            return

        self._exchange.handle_update_order(command)

    cpdef void cancel_order(self, CancelOrder command) except *:
        """
        Cancel the order with the `ClientOrderId` contained in the given command.

        Parameters
        ----------
        command : CancelOrder
            The command to execute.

        """
        if not self.is_connected:  # Simulate connection behaviour
            self._log.error(f"Cannot send command (not connected), {command}.")
            return

        self._exchange.handle_cancel_order(command)
