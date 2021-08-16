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
from typing import Dict

import orjson

from nautilus_trader.adapters.betfair.providers cimport BetfairInstrumentProvider
from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.live.data_client cimport LiveMarketDataClient
from nautilus_trader.model.c_enums.book_level cimport BookLevel
from nautilus_trader.model.data.base cimport Data
from nautilus_trader.model.data.base cimport DataType
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.betting cimport BettingInstrument
from nautilus_trader.msgbus.bus cimport MessageBus

from nautilus_trader.adapters.betfair.client import BetfairClient
from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.data_types import InstrumentSearch
from nautilus_trader.adapters.betfair.parsing import on_market_update
from nautilus_trader.adapters.betfair.sockets import BetfairMarketStreamClient


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
        loop not None: asyncio.AbstractEventLoop,
        client not None: BetfairClient,
        MessageBus msgbus not None,
        Cache cache not None,
        LiveClock clock not None,
        Logger logger not None,
        dict market_filter not None,
        BetfairInstrumentProvider instrument_provider not None,
        bint strict_handling=False,
    ):
        """
        Initialize a new instance of the ``BetfairDataClient`` class.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        client : BetfairClient
            The betfair HTTPClient
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.

        """
        self._client: BetfairClient = client
        self._instrument_provider: BetfairInstrumentProvider = instrument_provider or BetfairInstrumentProvider(
            client=client,
            logger=logger,
            market_filter=market_filter
        )
        super().__init__(
            loop=loop,
            client_id=ClientId(BETFAIR_VENUE.value),
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )
        self._stream = BetfairMarketStreamClient(
            client=self._client,
            logger=logger,
            message_handler=self.on_market_update,
        )

        self.subscription_status = SubscriptionStatus.UNSUBSCRIBED

        # Subscriptions
        self._subscribed_instrument_ids = set()  # type: set[InstrumentId]
        self._strict_handling = strict_handling
        self._subscribed_market_ids = set()   # type: set[InstrumentId]

    cpdef void _start(self) except *:
        self._log.info("Connecting...")
        self._loop.create_task(self._connect())

    async def _connect(self):
        self._log.info("Connecting to BetfairClient...")
        await self._client.connect()
        self._log.info("BetfairClient login successful.", LogColor.GREEN)

        # Connect market data socket
        await self._stream.connect()

        # Pass any preloaded instruments into the engine
        instruments = self._instrument_provider.list_instruments()
        if not instruments:
            await self._instrument_provider.load_all_async()
        instruments = self._instrument_provider.list_instruments()
        self._log.debug(f"Loading {len(instruments)} instruments from provider into cache, ")
        for instrument in instruments:
            self._handle_data(instrument)
            self._cache.add_instrument(instrument)

        self._log.debug(f"DataEngine has {len(self._cache.instruments(BETFAIR_VENUE))} Betfair instruments")

        # Schedule a heartbeat in 10s to give us a little more time to load instruments
        self._log.debug("scheduling heartbeat")
        self._loop.create_task(self._post_connect_heartbeat())

        self.is_connected = True
        self._log.info("Connected.")

    async def _post_connect_heartbeat(self):
        for _ in range(3):
            await asyncio.sleep(5)
            await self._stream.send(orjson.dumps({'op': 'heartbeat'}))

    cpdef void _stop(self) except *:
        self._loop.create_task(self._disconnect())

    async def _disconnect(self):
        self._log.info("Disconnecting...")

        # Close socket
        self._log.info("Closing streaming socket...")
        await self._stream.disconnect()

        # Ensure client closed
        self._log.info("Closing BetfairClient...")
        self._client.client_logout()

        self.is_connected = False
        self._log.info("Disconnected.")

    cpdef void _reset(self) except *:
        if self.is_connected:
            self._log.error("Cannot reset a connected data client.")
            return

        self._subscribed_instrument_ids = set()

    cpdef void _dispose(self) except *:
        if self.is_connected:
            self._log.error("Cannot dispose a connected data client.")
            return

# -- REQUESTS --------------------------------------------------------------------------------------

    cpdef void request(self, DataType data_type, UUID correlation_id) except *:
        if data_type.type == InstrumentSearch:
            # Strategy has requested a list of instruments
            self._loop.create_task(self._handle_instrument_search(data_type=data_type, correlation_id=correlation_id))
        else:
            super().request(data_type=data_type, correlation_id=correlation_id)

    async def _handle_instrument_search(self, data_type: DataType, correlation_id: UUID):
        await self._instrument_provider.load_all_async(market_filter=data_type.metadata)
        instruments = self._instrument_provider.search_instruments(instrument_filter=data_type.metadata)
        now = self._clock.timestamp_ns()
        search = InstrumentSearch(
            instruments=instruments,
            ts_event=now,
            ts_init=now,
        )
        self._handle_data_response(
            data_type=data_type,
            data=search,
            correlation_id=correlation_id
        )

# -- SUBSCRIPTIONS ---------------------------------------------------------------------------------

    cpdef void subscribe_order_book_deltas(
        self, InstrumentId instrument_id,
        BookLevel level,
        dict kwargs=None,
    ) except *:
        """
        Subscribe to `OrderBook` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to subscribe to.
        level : BookLevel
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
        self._subscribed_instrument_ids.add(instrument.id)
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

    cpdef void subscribe_trade_ticks(self, InstrumentId instrument_id) except *:
        pass  # Subscribed as part of orderbook

    cpdef void subscribe_instrument(self, InstrumentId instrument_id) except *:
        for instrument in self._instrument_provider.list_instruments():
            self._handle_data(data=instrument)

    cpdef void subscribe_instrument_status_updates(self, InstrumentId instrument_id) except *:
        pass  # Subscribed as part of orderbook

    cpdef void subscribe_instrument_close_prices(self, InstrumentId instrument_id) except *:
        pass  # Subscribed as part of orderbook

    cpdef void unsubscribe_order_book_snapshots(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `OrderBook` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to unsubscribe from.

        """
        Condition.not_none(instrument_id, "instrument_id")

        # TODO - this could be done by removing the market from self.__subscribed_market_ids and resending the
        #  subscription message - when we have a use case

        self._log.warning(f"Betfair does not support unsubscribing from instruments")

    cpdef void unsubscribe_order_book_deltas(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `OrderBook` data for the given instrument ID.

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
    cpdef void on_market_update(self, bytes raw) except *:
        cdef dict update = orjson.loads(raw)  # type: dict
        self._on_market_update(update=update)

    cpdef void _on_market_update(self, dict update) except *:
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
                if self._strict_handling:
                    if hasattr(data, "instrument_id") and data.instrument_id not in self._subscribed_instrument_ids:
                        # We receive data for multiple instruments within a subscription, don't emit data if we're not
                        # subscribed to this particular instrument as this will trigger a bunch of error logs
                        continue
                self._handle_data(data=data)
            elif isinstance(data, Event):
                self._log.warning(f"Received event: {data}, DataEngine not yet setup to send events")
