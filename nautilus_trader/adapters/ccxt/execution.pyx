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

import asyncio

import ccxt

from nautilus_trader.adapters.ccxt.providers import CCXTInstrumentProvider
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.model.c_enums.currency_type cimport CurrencyType
# from nautilus_trader.model.commands cimport CancelOrder
# from nautilus_trader.model.commands cimport ModifyOrder
# from nautilus_trader.model.commands cimport SubmitBracketOrder
# from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.events cimport AccountState
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.objects cimport Money
from nautilus_trader.live.execution cimport LiveExecutionClient
from nautilus_trader.live.execution cimport LiveExecutionEngine

cdef int _SECONDS_IN_HOUR = 60 * 60


cdef class CCXTExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for the unified CCXT Pro API.
    """

    def __init__(
        self,
        client not None: ccxt.Exchange,
        AccountId account_id not None,
        LiveExecutionEngine engine not None,
        LiveClock clock not None,
        Logger logger not None,
    ):
        """
        Initialize a new instance of the `CCXTExecutionClient` class.

        Parameters
        ----------
        client : ccxt.Exchange
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

        self._currencies = {}  # type: dict[str, Currency]

        # Scheduled tasks
        self._update_instruments_task = None
        self._watch_balances_task = None

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

        if self._client.check_required_credentials():
            self._log.info("API credentials validated.")
        else:
            self._log.error("API credentials missing or invalid.")
            self._log.error(f"Required: {self._client.required_credentials()}.")

        # Schedule instruments update
        delay = _SECONDS_IN_HOUR
        update = self._run_after_delay(delay, self._instruments_update(delay))
        self._update_instruments_task = self._loop.create_task(update)

        self._loop.create_task(self._connect())
        self._watch_balances_task = self._loop.create_task(self._watch_balances())

    async def _connect(self):
        await self._load_instruments()
        await self._load_currencies()
        await self._update_balances()

        self._is_connected = True
        self.initialized = True

        self._log.info("Connected.")

    cpdef void disconnect(self) except *:
        """
        Disconnect the client.
        """
        self._loop.create_task(self._disconnect())

    async def _disconnect(self):
        self._log.info("Disconnecting...")

        stop_tasks = []
        # Cancel update instruments
        if self._update_instruments_task:
            self._update_instruments_task.cancel()
            # TODO: This task is not finishing
            # stop_tasks.append(self._update_instruments_task)

        if self._watch_balances_task:
            self._watch_balances_task.cancel()
            # TODO: CCXT Pro issues for exchange.close()
            # stop_tasks.append(self._watch_balances_task)

        if stop_tasks:
            await asyncio.gather(*stop_tasks)

        # Ensure ccxt streams closed
        self._log.info("Closing web sockets...")
        await self._client.close()

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

    async def _run_after_delay(self, double delay, coro):
        await asyncio.sleep(delay)
        return await coro

    async def _load_instruments(self):
        await self._instrument_provider.load_all_async()
        self._log.info(f"Updated {self._instrument_provider.count} instruments.")

    async def _instruments_update(self, delay):
        await self._load_instruments()

        # Reschedule instruments update
        update = self._run_after_delay(delay, self._instruments_update(delay))
        self._update_instruments_task = self._loop.create_task(update)

    async def _load_currencies(self):
        try:
            response = await self._client.fetch_currencies()
        except TypeError:
            # Temporary workaround for testing
            response = self._client.fetch_currencies
        except Exception as ex:
            self._log.exception(ex)
            return

        cdef str code
        cdef dict values
        for code, values in response.items():
            currency_type = CurrencyType.FIAT if Currency.is_fiat_c(code) else CurrencyType.CRYPTO
            currency = Currency(
                code=code,
                precision=values["precision"],
                currency_type=currency_type,
            )

            self._currencies[code] = currency

        self._log.info(f"Updated {len(self._currencies)} currencies.")

    async def _update_balances(self):
        if not self._client.has["fetchBalance"]:
            self._log.error("`fetch_balance` not available.")
        try:
            response = await self._client.fetch_balance({'type': 'spot'})
        except TypeError:
            # Temporary workaround for testing
            response = self._client.fetch_balance
        except Exception as ex:
            self._log.error(f"{type(ex).__name__}: {ex} in _update_balances")
            return

        self._on_account_state(response)

    async def _watch_balances(self):
        cdef dict params = {'type': 'spot'}  # TODO: Hard coded for now
        cdef bint exiting = False  # Flag to stop loop
        try:
            while True:
                try:
                    response = await self._client.watch_balance(params)
                except TypeError:
                    # Temporary workaround for testing
                    response = self._client.watch_balance
                    exiting = True

                if response:
                    self._on_account_state(response)

                if exiting:
                    break
        except asyncio.CancelledError as ex:
            self._log.debug(f"Task cancelled `_watch_balances` for {self.account_id}.")
        except Exception as ex:
            self._log.error(f"{type(ex).__name__}: {ex} in _watch_balances")
        finally:
            # Finally close stream
            await self._client.close()

    cdef inline void _on_account_state(self, dict response) except *:
        cdef list balances = []
        cdef list balances_free = []
        cdef list balances_locked = []

        cdef str code
        cdef double amount
        cdef Currency currency

        # Update total balances
        for code, amount in response["total"].items():
            if amount == 0:
                continue
            currency = self._currencies.get(code)
            if currency is None:
                self._log.error(f"Cannot update total balance for {code} (no currency loaded).")
            balances.append(Money(amount, currency))

        # Update free balances
        for code, amount in response["free"].items():
            if amount == 0:
                continue
            currency = self._currencies.get(code)
            if currency is None:
                self._log.error(f"Cannot update total balance for {code} (no currency loaded).")
            balances_free.append(Money(amount, currency))

        # Update locked balances
        for code, amount in response["used"].items():
            if amount == 0:
                continue
            currency = self._currencies.get(code)
            if currency is None:
                self._log.error(f"Cannot update total balance for {code} (no currency loaded).")
            balances_locked.append(Money(amount, currency))

        # Generate event
        cdef AccountState event = AccountState(
            self.account_id,
            balances,
            balances_free,
            balances_locked,
            {},
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self._handle_event(event)
