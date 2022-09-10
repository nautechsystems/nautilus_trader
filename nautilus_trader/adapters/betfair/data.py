# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import Dict, Optional, Set

import msgspec

from nautilus_trader.adapters.betfair.client.core import BetfairClient
from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.data_types import InstrumentSearch
from nautilus_trader.adapters.betfair.data_types import SubscriptionStatus
from nautilus_trader.adapters.betfair.parsing import on_market_update
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.sockets import BetfairMarketStreamClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LogColor
from nautilus_trader.common.logging import Logger
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.data import Data
from nautilus_trader.core.message import Event
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data.base import DataType
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments.betting import BettingInstrument
from nautilus_trader.msgbus.bus import MessageBus


class BetfairDataClient(LiveMarketDataClient):
    """
    Provides a data client for the Betfair API.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : BetfairClient
        The betfair HttpClient
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    logger : Logger
        The logger for the client.
    market_filter : dict
        The market filter.
    instrument_provider : BetfairInstrumentProvider, optional
        The instrument provider.
    strict_handling : bool
        If strict handling mode is enabled.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: BetfairClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
        market_filter: Dict,
        instrument_provider: Optional[BetfairInstrumentProvider] = None,
        strict_handling: bool = False,
    ):
        super().__init__(
            loop=loop,
            client_id=ClientId(BETFAIR_VENUE.value),
            venue=BETFAIR_VENUE,
            instrument_provider=instrument_provider
            or BetfairInstrumentProvider(client=client, logger=logger, filters=market_filter),
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )

        self._instrument_provider: BetfairInstrumentProvider = instrument_provider
        self._client: BetfairClient = client
        self._stream = BetfairMarketStreamClient(
            client=self._client,
            logger=logger,
            message_handler=self.on_market_update,
        )

        self.subscription_status = SubscriptionStatus.UNSUBSCRIBED

        # Subscriptions
        self._subscribed_instrument_ids: Set[InstrumentId] = set()
        self._strict_handling = strict_handling
        self._subscribed_market_ids: Set[InstrumentId] = set()

    @property
    def instrument_provider(self) -> BetfairInstrumentProvider:
        return self._instrument_provider

    def connect(self):
        self._log.info("Connecting...")
        self._loop.create_task(self._connect())

    def disconnect(self):
        self._log.info("Disconnecting...")
        self._loop.create_task(self._disconnect())

    async def _connect(self):
        self._log.info("Connecting to BetfairClient...")
        await self._client.connect()
        self._log.info("BetfairClient login successful.", LogColor.GREEN)

        # Connect market data socket
        await self._stream.connect()

        # Pass any preloaded instruments into the engine
        if self._instrument_provider.count == 0:
            await self._instrument_provider.load_all_async()
        instruments = self._instrument_provider.list_all()
        self._log.debug(f"Loading {len(instruments)} instruments from provider into cache, ")
        for instrument in instruments:
            self._handle_data(instrument)

        self._log.debug(
            f"DataEngine has {len(self._cache.instruments(BETFAIR_VENUE))} Betfair instruments"
        )

        # Schedule a heartbeat in 10s to give us a little more time to load instruments
        self._log.debug("scheduling heartbeat")
        self._loop.create_task(self._post_connect_heartbeat())

        self._set_connected(True)
        self._log.info("Connected.")

    async def _post_connect_heartbeat(self):
        for _ in range(3):
            await asyncio.sleep(5)
            await self._stream.send(msgspec.json.encode({"op": "heartbeat"}))

    async def _disconnect(self):
        # Close socket
        self._log.info("Closing streaming socket...")
        await self._stream.disconnect()

        # Ensure client closed
        self._log.info("Closing BetfairClient...")
        self._client.client_logout()

        self._set_connected(False)
        self._log.info("Disconnected.")

    def _reset(self):
        if self.is_connected:
            self._log.error("Cannot reset a connected data client.")
            return

        self._subscribed_instrument_ids = set()

    def _dispose(self):
        if self.is_connected:
            self._log.error("Cannot dispose a connected data client.")
            return

    # -- REQUESTS ---------------------------------------------------------------------------------

    def request(self, data_type: DataType, correlation_id: UUID4):
        if data_type.type == InstrumentSearch:
            # Strategy has requested a list of instruments
            self._loop.create_task(
                self._handle_instrument_search(data_type=data_type, correlation_id=correlation_id)
            )
        else:
            super().request(data_type=data_type, correlation_id=correlation_id)

    async def _handle_instrument_search(self, data_type: DataType, correlation_id: UUID4):
        await self._instrument_provider.load_all_async(market_filter=data_type.metadata)
        instruments = self._instrument_provider.search_instruments(
            instrument_filter=data_type.metadata
        )
        now = self._clock.timestamp_ns()
        search = InstrumentSearch(
            instruments=instruments,
            ts_event=now,
            ts_init=now,
        )
        self._handle_data_response(data_type=data_type, data=search, correlation_id=correlation_id)

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    def subscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Optional[int] = None,
        kwargs: Optional[Dict] = None,
    ):
        if kwargs is None:
            kwargs = {}
        PyCondition.not_none(instrument_id, "instrument_id")

        instrument: BettingInstrument = self._instrument_provider.find(instrument_id)

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

        self._log.info(
            f"Added market_id {instrument.market_id} for {instrument_id.symbol} <OrderBook> data."
        )

    async def delayed_subscribe(self, delay=0):
        self._log.debug(f"Scheduling subscribe for delay={delay}")
        await asyncio.sleep(delay)
        self._log.info(f"Sending subscribe for market_ids {self._subscribed_market_ids}")
        await self._stream.send_subscription_message(market_ids=list(self._subscribed_market_ids))
        self._log.info(f"Added market_ids {self._subscribed_market_ids} for <OrderBookData> data.")

    def subscribe_trade_ticks(self, instrument_id: InstrumentId):
        pass  # Subscribed as part of orderbook

    def subscribe_instrument(self, instrument_id: InstrumentId):
        for instrument in self._instrument_provider.list_all():
            self._handle_data(data=instrument)

    def subscribe_instrument_status_updates(self, instrument_id: InstrumentId):
        pass  # Subscribed as part of orderbook

    def subscribe_instrument_close_prices(self, instrument_id: InstrumentId):
        pass  # Subscribed as part of orderbook

    def unsubscribe_order_book_snapshots(self, instrument_id: InstrumentId):
        # TODO - this could be done by removing the market from self.__subscribed_market_ids and resending the
        #  subscription message - when we have a use case

        self._log.warning("Betfair does not support unsubscribing from instruments")

    def unsubscribe_order_book_deltas(self, instrument_id: InstrumentId):
        # TODO - this could be done by removing the market from self.__subscribed_market_ids and resending the
        #  subscription message - when we have a use case
        self._log.warning("Betfair does not support unsubscribing from instruments")

    # -- INTERNAL ---------------------------------------------------------------------------------

    def _log_betfair_error(self, ex: Exception, method_name: str):
        self._log.warning(f"{type(ex).__name__}: {ex} in {method_name}")

    def handle_data(self, data: Data):
        self._handle_data(data=data)

    # -- STREAMS ----------------------------------------------------------------------------------
    def on_market_update(self, raw: bytes):
        update = msgspec.json.decode(raw)
        self._on_market_update(update=update)

    def _on_market_update(self, update):
        if self._check_stream_unhealthy(update=update):
            pass
        updates = on_market_update(
            instrument_provider=self._instrument_provider,
            update=update,
        )
        if not updates:
            self._handle_no_data(update=update)
        for data in updates:
            self._log.debug(f"{data}")
            if isinstance(data, Data):
                if self._strict_handling:
                    if (
                        hasattr(data, "instrument_id")
                        and data.instrument_id not in self._subscribed_instrument_ids
                    ):
                        # We receive data for multiple instruments within a subscription, don't emit data if we're not
                        # subscribed to this particular instrument as this will trigger a bunch of error logs
                        continue
                self._handle_data(data=data)
            elif isinstance(data, Event):
                self._log.warning(
                    f"Received event: {data}, DataEngine not yet setup to send events"
                )

    def _check_stream_unhealthy(self, update: Dict):
        conflated = update.get("con", False)  # Consuming data slower than the rate of deliver
        if conflated:
            self._log.warning(
                "Conflated stream - consuming data too slow (data received is delayed)"
            )
        if update.get("status") == 503:
            self._log.warning("Stream unhealthy, waiting for recover")
            self.degrade()

    def _handle_no_data(self, update):
        if update.get("op") == "connection" or update.get("connectionsAvailable"):
            return
        if update.get("status") == 503:
            # handled in `_check_stream_unhealthy`
            return
        if update.get("ct") == "HEARTBEAT":
            if self.is_degraded:
                self.resume()
            return
        self._log.warning(f"Received message but parsed no updates: {update}")
        if update.get("statusCode") == "FAILURE" and update.get("connectionClosed"):
            # TODO (bm) - self._loop.create_task(self._stream.reconnect())
            self._log.error(str(update))
            raise RuntimeError()
