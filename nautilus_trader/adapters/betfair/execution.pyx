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

import betfairlightweight

from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger

from nautilus_trader.core.message cimport Event
from nautilus_trader.live.execution_client cimport LiveExecutionClient
from nautilus_trader.live.execution_engine cimport LiveExecutionEngine
from nautilus_trader.model.commands cimport AmendOrder
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport SubmitOrder

from nautilus_trader.model.identifiers cimport AccountId

from nautilus_trader.adapters.betfair.providers cimport BetfairInstrumentProvider

from adapters.betfair.common import order_submit_to_betfair, order_amend_to_betfair, order_cancel_to_betfair

cdef int _SECONDS_IN_HOUR = 60 * 60


cdef class BetfairExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for Betfair.
    """

    def __init__(
        self,
        client not None: betfairlightweight.APIClient,
        AccountId account_id not None,
        LiveExecutionEngine engine not None,
        LiveClock clock not None,
        Logger logger not None,
    ):
        """
        Initialize a new instance of the `BetfairExecutionClient` class.

        Parameters
        ----------
        client : betfairlightweight.APIClient
            The Betfair client.
        account_id : AccountId
            The account identifier for the client.
        engine : LiveDataEngine
            The data engine for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.

        """
        cdef BetfairInstrumentProvider instrument_provider = BetfairInstrumentProvider(
            client=client,
            load_all=False,
        )

        super().__init__(
            "BETFAIR",
            account_id,
            engine,
            instrument_provider,
            clock,
            logger,
            config={
                "name": "BetfairExecClient",
            }
        )

        self._client = client # type: betfairlightweight.APIClient
        self.is_connected = False

    cpdef void connect(self) except *:
        self._log.info("Connecting...")
        self._client.login()
        self._log.info("APIClient login successful.", LogColor.GREEN)

        self._log.info("Loading Instruments.")
        self._instrument_provider.load_all()
        self._log.info(f"Loaded {len(self._instrument_provider._instruments)} Instruments.")

        self.is_connected = True
        self._log.info("Connected.")

    def _connect_order_stream(self):
        """

        :return:
        """
        pass

    cpdef void disconnect(self) except *:
        self._client.client_logout()
        self._log.info("Disconnected.")

    # -- COMMAND HANDLERS ------------------------------------------------------------------------------
    # TODO - Add support for bulk updates - betfair allows up to 200 inserts / 60 updates / 60 cancels per request

    cpdef void submit_order(self, SubmitOrder command) except *:
        instrument = self._instrument_provider._instruments[command.instrument_id]
        kw = order_submit_to_betfair(command=command, instrument=instrument)
        self._client.betting.place_orders(**kw)

    cpdef void amend_order(self, AmendOrder command) except *:
        # TODO - Need to know instrument_id
        instrument = self._instrument_provider._instruments[command.instrument_id]
        kw = order_amend_to_betfair(command=command)
        self._client.betting.replace_orders(**kw)

    cpdef void cancel_order(self, CancelOrder command) except *:
        instrument = self._instrument_provider._instruments[command.instrument_id]
        kw = order_cancel_to_betfair(command=command)
        self._client.betting.cancel_orders(**kw)

    # -- PYTHON WRAPPERS -------------------------------------------------------------------------------

    def _handle_event_py(self, event: Event):
        self._engine.process(event)

    # -- EVENT HANDLERS --------------------------------------------------------------------------------

    cdef void _handle_event(self, Event event) except *:
        self._engine.process(event)
