# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import Optional

import msgspec
from betfair_parser.spec.streaming import MCM
from betfair_parser.spec.streaming import Connection
from betfair_parser.spec.streaming import Status
from betfair_parser.spec.streaming import stream_decode

from nautilus_trader.adapters.betfair.client import BetfairHttpClient
from nautilus_trader.adapters.betfair.constants import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.data_types import BetfairStartingPrice
from nautilus_trader.adapters.betfair.data_types import BSPOrderBookDelta
from nautilus_trader.adapters.betfair.data_types import SubscriptionStatus
from nautilus_trader.adapters.betfair.parsing.core import BetfairParser
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.sockets import BetfairMarketStreamClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.logging import Logger
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.data import Data
from nautilus_trader.core.message import Event
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data.base import DataType
from nautilus_trader.model.data.base import GenericData
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
    instrument_provider : BetfairInstrumentProvider, optional
        The instrument provider.
    strict_handling : bool
        If strict handling mode is enabled.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: BetfairHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
        instrument_provider: BetfairInstrumentProvider,
        account_currency: str,
        strict_handling: bool = False,
    ):
        super().__init__(
            loop=loop,
            client_id=ClientId(BETFAIR_VENUE.value),
            venue=BETFAIR_VENUE,
            instrument_provider=instrument_provider,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )

        self._instrument_provider: BetfairInstrumentProvider = instrument_provider
        self._client: BetfairHttpClient = client
        self._stream = BetfairMarketStreamClient(
            http_client=self._client,
            logger=logger,
            message_handler=self.on_market_update,
        )
        self.parser = BetfairParser(currency=account_currency)
        self.subscription_status = SubscriptionStatus.UNSUBSCRIBED

        # Subscriptions
        self._subscribed_instrument_ids: set[InstrumentId] = set()
        self._strict_handling = strict_handling
        self._subscribed_market_ids: set[InstrumentId] = set()

    @property
    def instrument_provider(self) -> BetfairInstrumentProvider:
        return self._instrument_provider

    async def _connect(self):
        self._log.info("Connecting to BetfairHttpClient...")
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
            f"DataEngine has {len(self._cache.instruments(BETFAIR_VENUE))} Betfair instruments",
        )

        # Schedule a heartbeat in 10s to give us a little more time to load instruments
        self._log.debug("scheduling heartbeat")
        self.create_task(self._post_connect_heartbeat())

    async def _post_connect_heartbeat(self):
        for _ in range(3):
            await self._stream.send(msgspec.json.encode({"op": "heartbeat"}))
            await asyncio.sleep(5)

    async def _disconnect(self):
        # Close socket
        self._log.info("Closing streaming socket...")
        await self._stream.disconnect()

        # Ensure client closed
        self._log.info("Closing BetfairClient...")
        await self._client.disconnect()

    def _reset(self):
        if self.is_connected:
            self._log.error("Cannot reset a connected data client.")
            return

        self._subscribed_instrument_ids = set()

    def _dispose(self):
        if self.is_connected:
            self._log.error("Cannot dispose a connected data client.")
            return

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    async def _subscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Optional[int] = None,
        kwargs: Optional[dict] = None,
    ):
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
            self.create_task(self.delayed_subscribe(delay=3))
            self.subscription_status = SubscriptionStatus.PENDING_STARTUP
        elif self.subscription_status == SubscriptionStatus.PENDING_STARTUP:
            pass
        elif self.subscription_status == SubscriptionStatus.RUNNING:
            self.create_task(self.delayed_subscribe(delay=0))

        self._log.info(
            f"Added market_id {instrument.market_id} for {instrument_id.symbol} <OrderBook> data.",
        )

    async def delayed_subscribe(self, delay=0):
        self._log.debug(f"Scheduling subscribe for delay={delay}")
        await asyncio.sleep(delay)
        self._log.info(f"Sending subscribe for market_ids {self._subscribed_market_ids}")
        await self._stream.send_subscription_message(market_ids=list(self._subscribed_market_ids))
        self._log.info(f"Added market_ids {self._subscribed_market_ids} for <OrderBook> data.")

    async def _subscribe_ticker(self, instrument_id: InstrumentId) -> None:
        pass  # Subscribed as part of orderbook

    async def _subscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        pass  # Subscribed as part of orderbook

    async def _subscribe_instrument(self, instrument_id: InstrumentId) -> None:
        self._log.info("Skipping subscribe_instrument, betfair subscribes as part of orderbook")
        return

    async def _subscribe_instruments(self) -> None:
        for instrument in self._instrument_provider.list_all():
            self._handle_data(instrument)

    async def _subscribe_instrument_status(self, instrument_id: InstrumentId):
        pass  # Subscribed as part of orderbook

    async def _subscribe_instrument_close(self, instrument_id: InstrumentId):
        pass  # Subscribed as part of orderbook

    async def _unsubscribe_order_book_snapshots(self, instrument_id: InstrumentId):
        # TODO - this could be done by removing the market from self.__subscribed_market_ids and resending the
        #  subscription message - when we have a use case

        self._log.warning("Betfair does not support unsubscribing from instruments")

    async def _unsubscribe_order_book_deltas(self, instrument_id: InstrumentId):
        # TODO - this could be done by removing the market from self.__subscribed_market_ids and resending the
        #  subscription message - when we have a use case
        self._log.warning("Betfair does not support unsubscribing from instruments")

    # -- STREAMS ----------------------------------------------------------------------------------
    def on_market_update(self, raw: bytes):
        update = stream_decode(raw)
        if isinstance(update, MCM):
            self._on_market_update(mcm=update)
        elif isinstance(update, Connection):
            pass
        elif isinstance(update, Status):
            self._handle_status_message(update=update)
        else:
            raise RuntimeError

    def _on_market_update(self, mcm: MCM):
        self._check_stream_unhealthy(update=mcm)
        updates = self.parser.parse(mcm=mcm)
        for data in updates:
            self._log.debug(f"{data}")
            if isinstance(data, (BetfairStartingPrice, BSPOrderBookDelta)):
                # Not a regular data type
                generic_data = GenericData(
                    DataType(data.__class__, {"instrument_id": data.instrument_id}),
                    data,
                )
                self._handle_data(generic_data)
            elif isinstance(data, Data):
                if self._strict_handling and (
                    hasattr(data, "instrument_id")
                    and data.instrument_id not in self._subscribed_instrument_ids
                ):
                    # We receive data for multiple instruments within a subscription, don't emit data if we're not
                    # subscribed to this particular instrument as this will trigger a bunch of error logs
                    continue
                self._handle_data(data)
            elif isinstance(data, Event):
                self._log.warning(
                    f"Received event: {data}, DataEngine not yet setup to send events",
                )
            else:
                raise RuntimeError

    def _check_stream_unhealthy(self, update: MCM):
        if update.stream_unreliable:
            self._log.warning("Stream unhealthy, waiting for recover")
            self.degrade()
        if update.mc is not None:
            for mc in update.mc:
                if mc.con:
                    self._log.warning(
                        "Conflated stream - consuming data too slow (data received is delayed)",
                    )

    def _handle_status_message(self, update: Status):
        if update.status_code == "FAILURE" and update.connection_closed:
            self._log.warning(str(update))
            if update.error_code == "MAX_CONNECTION_LIMIT_EXCEEDED":
                raise RuntimeError("No more connections available")
            else:
                self._log.info("Attempting reconnect")
                self.create_task(self._stream.connect())
