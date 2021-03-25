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
from betfairlightweight import APIClient

from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.live.data_client cimport LiveMarketDataClient
from nautilus_trader.live.data_engine cimport LiveDataEngine
from nautilus_trader.model.data cimport DataType
from nautilus_trader.model.data cimport GenericData
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instrument cimport BettingInstrument
from nautilus_trader.adapters.betfair.parsing import on_market_update
from nautilus_trader.adapters.betfair.providers cimport BetfairInstrumentProvider
from nautilus_trader.model.data cimport Data

from nautilus_trader.adapters.betfair.sockets import BetfairMarketStreamClient


cdef int _SECONDS_IN_HOUR = 60 * 60

class InstrumentSearch:
    def __init__(self, instruments):
        self.instruments = instruments


# Notes
# TODO - if you receive con=true flag on a market - then you are consuming data slower than the rate of deliver. If the
#  socket buffer is full we won't attempt to push; so the next push will be conflated.
#  We should warn about this.

# TODO - Betfair reports status:503 in messages if the stream is unhealthy. We should send out a warning / health
#  message, potentially letting strategies know to temporarily "pause" ?

# TODO - segmentationEnabled=true segmentation breaks up large messages and improves: end to end performance, latency,
#  time to first and last byte


cdef class BetfairDataClient(LiveMarketDataClient):
    """
    Provides a data client for the Betfair API.
    """

    def __init__(
        self,
        betfairlightweight.APIClient client not None,
        LiveDataEngine engine not None,
        LiveClock clock not None,
        Logger logger not None,
        dict market_filter not None,
    ):
        """
        Initialize a new instance of the `BetfairDataClient` class.

        Parameters
        ----------
        client : betfairlightweight.APIClient
            The betfairlightweight client.
        engine : LiveDataEngine
            The live data engine for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.

        Raises
        ------
        """
        cdef BetfairInstrumentProvider instrument_provider = BetfairInstrumentProvider(
            client=client,
            load_all=True,
            market_filter=market_filter
        )
        super().__init__(
            "BetfairDataClient",
            engine,
            clock,
            logger,
        )

        self._client = client  # type: APIClient
        self._instrument_provider = instrument_provider
        self._stream = BetfairMarketStreamClient(
            client=self._client, message_handler=self._on_market_update,
        )
        self.is_connected = False

        # Subscriptions
        self._subscribed_market_ids = set()      # type: set[InstrumentId]

        # Scheduled tasks
        self._update_instruments_task = None

    cpdef void connect(self) except *:
        """
        Connect the client.
        """
        self._log.info("Connecting...")
        self._loop.create_task(self._connect())

    async def _connect(self):
        self._log.info("Connecting to Betfair APIClient...")
        resp = self._client.login()
        self._log.info("Betfair APIClient login successful.", LogColor.GREEN)

        # Connect market data socket
        await self._stream.connect()

        self.is_connected = True
        self._log.info("Connected.")

    cpdef void disconnect(self) except *:
        """
        Disconnect the client.
        """
        self._loop.create_task(self._disconnect())

    async def _disconnect(self):
        self._log.info("Disconnecting...")

        stop_tasks = []

        # Ensure client closed
        self._log.info("Closing APICClient(s)...")
        self._client.client_logout()

        self.is_connected = False
        self._log.info("Disconnected.")

    cpdef void reset(self) except *:
        """
        Reset the client.
        """
        if self.is_connected:
            self._log.error("Cannot reset a connected data client.")
            return

        self._log.info("Resetting...")

        # TODO: Reset client
        self._instrument_provider = BetfairInstrumentProvider(
            client=self._client,
            load_all=False,
        )

        self._subscribed_instruments = set()

        self._log.info("Reset.")

    cpdef void dispose(self) except *:
        """
        Dispose the client.
        """
        if self.is_connected:
            self._log.error("Cannot dispose a connected data client.")
            return

        self._log.info("Disposing...")

        # Nothing to dispose yet

        self._log.info("Disposed.")

# -- REQUESTS --------------------------------------------------------------------------------------

    cpdef void request(self, DataType data_type, UUID correlation_id) except *:
        print(data_type,data_type.type, InstrumentSearch,  data_type.type == InstrumentSearch)
        if data_type.type == InstrumentSearch:
            print("HANDLING")
            # Strategy has requested a list of instruments
            instruments = self._instrument_provider.search_instruments(instrument_filter=data_type.metadata)
            self._handle_data_response(
                data=GenericData(
                    data_type=data_type,
                    data=InstrumentSearch(instruments=instruments),
                    timestamp=self._clock.utc_now(),
                ),
                correlation_id=correlation_id
            )
        else:
            super().request(data_type=data_type, correlation_id=correlation_id)

    # -- SUBSCRIPTIONS ---------------------------------------------------------------------------------

    cpdef void subscribe_order_book(self, InstrumentId instrument_id, OrderBookLevel level, int depth=0, dict kwargs=None) except *:
        """
        Subscribe to `OrderBook` data for the given instrument identifier.

        Parameters
        ----------
        instrument_id : InstrumentId
            The Instrument id to subscribe to order books.
        depth : int, optional
            The maximum depth for the order book. A depth of 0 is maximum depth.
        kwargs : dict, optional
            The keyword arguments for exchange specific parameters.

        """
        if kwargs is None:
            kwargs = {}
        Condition.not_none(instrument_id, "instrument_id")

        cdef BettingInstrument instrument = self._instrument_provider.find(instrument_id)  # type: BettingInstrument

        if instrument.market_id  in self._subscribed_market_ids:
            self._log.warning(f"Already subscribed to market_id: {instrument.market_id} [Instrument: {instrument_id.symbol}] <OrderBook> data.")
            return

        self._stream.send_subscription_message(
            market_ids=[instrument.market_id]
        )

        self._log.info(f"Subscribed to market_id {instrument.market_id} for {instrument_id.symbol} <OrderBook> data.")

    cpdef void unsubscribe_order_book(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `OrderBook` data for the given instrument identifier.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to unsubscribe from.

        """
        Condition.not_none(instrument_id, "instrument_id")
        self._log.warning(f"Betfair does not support unsubscribing from instruments")

# -- INTERNAL --------------------------------------------------------------------------------------

    cdef inline void _log_betfair_error(self, ex, str method_name) except *:
        self._log.warning(f"{type(ex).__name__}: {ex} in {method_name}")


# -- Debugging ---------------------------------------------------------------------------------------

    cpdef BetfairInstrumentProvider instrument_provider(self):
        return self._instrument_provider

    cpdef void handle_data(self, Data data):
        self._handle_data(data=data)

# -- STREAMS ---------------------------------------------------------------------------------------

    cpdef void _on_market_update(self, dict update) except *:
        updates = on_market_update(raw=update, instrument_provider=self.instrument_provider())
        for upd in updates:
            self.handle_data(data=upd)
