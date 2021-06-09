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

from betfairlightweight import APIClient
import orjson

from nautilus_trader.adapters.betfair.providers cimport BetfairInstrumentProvider
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.live.data_client cimport LiveMarketDataClient
from nautilus_trader.live.data_engine cimport LiveDataEngine
from nautilus_trader.model.c_enums.orderbook_level cimport OrderBookLevel
from nautilus_trader.model.data cimport Data
from nautilus_trader.model.data cimport DataType
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.betting cimport BettingInstrument

from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.parsing import on_market_update
from nautilus_trader.adapters.betfair.sockets import BetfairMarketStreamClient


cdef int _SECONDS_IN_HOUR = 60 * 60


class InstrumentSearch(Data):
    def __init__(
        self,
        instruments,
        ts_event_ns,
        ts_recv_ns,
    ):
        super().__init__(ts_event_ns, ts_recv_ns)
        self.instruments = instruments


# Notes
# TODO - if you receive con=true flag on a market - then you are consuming data
#  slower than the rate of deliver. If the socket buffer is full we won't
#  attempt to push; so the next push will be conflated. We should warn about this.

# TODO - Betfair reports status:503 in messages if the stream is unhealthy.
#  We should send out a warning / health message, potentially letting strategies
#  know to temporarily "pause"?

# TODO - segmentationEnabled=true segmentation breaks up large messages and
#  improves: end to end performance, latency, time to first and last byte.


cdef class BetfairDataClient(LiveMarketDataClient):
    """
    Provides a data client for the Betfair API.
    """

    def __init__(
        self,
        client not None,
        LiveDataEngine engine not None,
        LiveClock clock not None,
        Logger logger not None,
        dict market_filter not None,
        bint load_instruments=True,
    ):
        """
        Initialize a new instance of the ``BetfairDataClient`` class.

        Parameters
        ----------
        client : APIClient
            The betfairlightweight client.
        engine : LiveDataEngine
            The live data engine for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.

        """

        self._client = client  # type: APIClient
        self._client.login()

        cdef BetfairInstrumentProvider instrument_provider = BetfairInstrumentProvider(
            client=client,
            logger=logger,
            load_all=load_instruments,
            market_filter=market_filter
        )
        super().__init__(
            client_id=ClientId(BETFAIR_VENUE.value),
            engine=engine,
            clock=clock,
            logger=logger,
        )
        self._instrument_provider = instrument_provider
        self._stream = BetfairMarketStreamClient(
            client=self._client, logger=logger, message_handler=self._on_market_update,
        )
        self.is_connected = False
        self.subscription_status = SubscriptionStatus.UNSUBSCRIBED

        # Subscriptions
        self._subscribed_instruments = set()  # type: set[InstrumentId]
        self._subscribed_market_ids = set()   # type: set[InstrumentId]

    cpdef void connect(self) except *:
        """
        Connect the client.
        """
        self._log.info("Connecting...")
        self._loop.create_task(self._connect())

    async def _connect(self):
        self._log.info("Connecting to Betfair APIClient...")
        self._client.login()
        self._log.info("Betfair APIClient login successful.", LogColor.GREEN)

        # Connect market data socket
        await self._stream.connect()

        # Pass any preloaded instruments into the engine
        for instrument in self._instrument_provider.get_all().values():
            self._handle_data(instrument)
            self._engine.cache.add_instrument(instrument)

        self._log.debug(f"DataEngine has {len(self._engine.cache.instruments(BETFAIR_VENUE))} Betfair instruments")

        # Schedule a heartbeat in 10s to give us a little more time to load instruments
        self._log.debug("scheduling heartbeat")
        self._loop.create_task(self._post_connect_heartbeat())

        self.is_connected = True
        self._log.info("Connected.")

    async def _post_connect_heartbeat(self):
        for _ in range(3):
            await asyncio.sleep(5)
            await self._stream.send(orjson.dumps({'op': 'heartbeat'}))

    cpdef void disconnect(self) except *:
        """
        Disconnect the client.
        """
        self._loop.create_task(self._disconnect())

    async def _disconnect(self):
        self._log.info("Disconnecting...")

        # Close socket
        self._log.info("Closing streaming socket...")
        await self._stream.disconnect()

        # Ensure client closed
        self._log.info("Closing APIClient...")
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

        # TODO: Reset client ?

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
        if data_type.type == InstrumentSearch:
            # Strategy has requested a list of instruments
            instruments = self._instrument_provider.search_instruments(instrument_filter=data_type.metadata)
            now = self._clock.timestamp_ns()
            search = InstrumentSearch(
                instruments=instruments,
                ts_event_ns=now,
                ts_recv_ns=now,
            )
            self._handle_data_response(
                data_type=data_type,
                data=search,
                correlation_id=correlation_id
            )
        else:
            super().request(data_type=data_type, correlation_id=correlation_id)

# -- SUBSCRIPTIONS ---------------------------------------------------------------------------------

    cpdef void subscribe_order_book(
        self, InstrumentId instrument_id,
        OrderBookLevel level,
        int depth=0,
        dict kwargs=None,
    ) except *:
        """
        Subscribe to `OrderBook` data for the given instrument identifier.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to subscribe to.
        level : OrderBookLevel
            The order book level (L1, L2, L3).
        depth : int, optional
            The maximum depth for the order book. A depth of 0 is maximum depth.
        kwargs : dict, optional
            The keyword arguments for exchange specific parameters.

        """
        if kwargs is None:
            kwargs = {}
        Condition.not_none(instrument_id, "instrument_id")

        cdef BettingInstrument instrument = self._instrument_provider.find(instrument_id)  # type: BettingInstrument

        if instrument.market_id in self._subscribed_market_ids:
            self._log.warning(
                f"Already subscribed to market_id: {instrument.market_id} "
                f"[Instrument: {instrument_id.symbol}] <OrderBook> data.",
            )
            return

        # If this is the first subscription request we're receiving, schedule a
        # subscription after a short delay to allow other strategies to send
        # their subscriptions (every change triggers a full snapshot).
        self._subscribed_market_ids.add(instrument.market_id)
        if self.subscription_status == SubscriptionStatus.UNSUBSCRIBED:
            self._loop.create_task(self.delayed_subscribe(delay=5))
            self.subscription_status = SubscriptionStatus.PENDING_STARTUP
        elif self.subscription_status == SubscriptionStatus.PENDING_STARTUP:
            pass
        elif self.subscription_status == SubscriptionStatus.RUNNING:
            self._loop.create_task(self.delayed_subscribe(delay=0))

        self._log.info(f"Added market_id {instrument.market_id} for {instrument_id.symbol} <OrderBook> data.")

    async def delayed_subscribe(self, delay=0):
        self._log.debug(f"Scheduling subscribe for delay={delay}")
        await asyncio.sleep(delay)
        self._log.info(f"Sending subscribe for market_ids {self._subscribed_market_ids}")
        await self._stream.send_subscription_message(market_ids=list(self._subscribed_market_ids))
        self._log.info(f"Added market_ids {self._subscribed_market_ids} for <OrderBookData> data.")

    cpdef void subscribe_order_book_deltas(self, InstrumentId instrument_id, OrderBookLevel level, dict kwargs=None) except *:
        self.subscribe_order_book(instrument_id=instrument_id, level=level, kwargs=kwargs)

    cpdef void unsubscribe_order_book(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `OrderBook` data for the given instrument identifier.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to unsubscribe from.

        """
        # TODO - this could be done by removing the market from self.__subscribed_market_ids and resending the
        #  subscription message - when we have a use case
        Condition.not_none(instrument_id, "instrument_id")
        self._log.warning(f"Betfair does not support unsubscribing from instruments")

    cpdef void unsubscribe_order_book_deltas(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `OrderBook` data for the given instrument identifier.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to unsubscribe from.

        """
        # TODO - this could be done by removing the market from self.__subscribed_market_ids and resending the
        #  subscription message - when we have a use case
        Condition.not_none(instrument_id, "instrument_id")
        self._log.warning(f"Betfair does not support unsubscribing from instruments")

# -- INTERNAL --------------------------------------------------------------------------------------

    cdef void _log_betfair_error(self, ex, str method_name) except *:
        self._log.warning(f"{type(ex).__name__}: {ex} in {method_name}")

# -- Debugging ---------------------------------------------------------------------------------------

    cpdef BetfairInstrumentProvider instrument_provider(self):
        return self._instrument_provider

    cpdef void handle_data(self, Data data) except *:
        self._handle_data(data=data)

# -- STREAMS ---------------------------------------------------------------------------------------

    cpdef void _on_market_update(self, bytes raw) except *:
        cdef dict update = orjson.loads(raw)  # type: dict
        updates = on_market_update(
            instrument_provider=self._instrument_provider,
            update=update,
        )
        if not updates:
            if update.get('op') == 'connection' or update.get('connectionsAvailable'):
                return
            self._log.warning(f"Received message but parsed no updates: {update}")
            if update.get("statusCode") == 'FAILURE' and update.get('connectionClosed'):
                # TODO - self._loop.create_task(self._stream.reconnect())
                self._log.error(str(update))
                raise RuntimeError()
        for data in updates:
            self._log.debug(f"{data}")
            if isinstance(data, Data):
                self._handle_data(data=data)
            elif isinstance(data, Event):
                self._log.warning(f"Received event: {data}, DataEngine not yet setup to send events")
