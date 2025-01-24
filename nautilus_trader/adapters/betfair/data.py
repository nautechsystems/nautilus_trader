# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import Any

import msgspec
from betfair_parser.spec.streaming import MCM
from betfair_parser.spec.streaming import Connection
from betfair_parser.spec.streaming import Status
from betfair_parser.spec.streaming import stream_decode

from nautilus_trader.adapters.betfair.client import BetfairHttpClient
from nautilus_trader.adapters.betfair.constants import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.data_types import SubscriptionStatus
from nautilus_trader.adapters.betfair.parsing.common import merge_instrument_fields
from nautilus_trader.adapters.betfair.parsing.core import BetfairParser
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.sockets import BetfairMarketStreamClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.data import Data
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments.betting import BettingInstrument
from nautilus_trader.model.objects import Currency


class BetfairDataClient(LiveMarketDataClient):
    """
    Provides a data client for the Betfair API.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : BetfairClient
        The Betfair HttpClient
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : BetfairInstrumentProvider, optional
        The instrument provider.
    account_currency : Currency
        The currency for the Betfair account.
    keep_alive_period : int, default 36_000 (10 hours)
        The keep alive period (seconds) for the socket client.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: BetfairHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: BetfairInstrumentProvider,
        account_currency: Currency,
        keep_alive_period: int = 3600 * 10,  # 10 hours
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(BETFAIR_VENUE.value),
            venue=BETFAIR_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )
        self._instrument_provider: BetfairInstrumentProvider = instrument_provider

        # Configuration
        self.keep_alive_period = keep_alive_period
        self._log.info(f"{keep_alive_period=}", LogColor.BLUE)

        # Clients
        self._client: BetfairHttpClient = client
        self._stream = BetfairMarketStreamClient(
            http_client=self._client,
            message_handler=self.on_market_update,
        )
        self._reconnect_in_progress = False

        self._parser = BetfairParser(currency=account_currency.code)
        self.subscription_status = SubscriptionStatus.UNSUBSCRIBED

        # Async tasks
        self._keep_alive_task: asyncio.Task | None = None

        # TODO: Move heartbeat down to Rust socket
        self._heartbeat_task: asyncio.Task | None = None

        # Subscriptions
        self._subscribed_instrument_ids: set[InstrumentId] = set()
        self._subscribed_market_ids: set[InstrumentId] = set()

    @property
    def instrument_provider(self) -> BetfairInstrumentProvider:
        return self._instrument_provider

    async def _connect(self) -> None:
        await self._client.connect()
        await self._stream.connect()

        # Pass any preloaded instruments into the engine
        if self._instrument_provider.count == 0:
            await self._instrument_provider.load_all_async()
        instruments = self._instrument_provider.list_all()
        self._log.debug(f"Loading {len(instruments)} instruments from provider into cache")
        for instrument in instruments:
            self._handle_data(instrument)

        self._log.debug(
            f"DataEngine has {len(self._cache.instruments(BETFAIR_VENUE))} Betfair instruments",
        )

        # Schedule a heartbeat in 10s to give us a little more time to load instruments
        self._log.debug("Scheduling heartbeat")
        if not self._heartbeat_task:
            self._heartbeat_task = self.create_task(self._post_connect_heartbeat())

        if not self._keep_alive_task:
            self._keep_alive_task = self.create_task(self._keep_alive())

        # Check for any global filters in instrument provider to subscribe
        if self.instrument_provider._config.event_type_ids:
            await self._stream.send_subscription_message(
                event_type_ids=self.instrument_provider._config.event_type_ids,
                country_codes=self.instrument_provider._config.country_codes,
                market_types=self.instrument_provider._config.market_types,
            )
            self.subscription_status = SubscriptionStatus.SUBSCRIBED

    async def _post_connect_heartbeat(self) -> None:
        try:
            for _ in range(3):
                try:
                    await self._stream.send(msgspec.json.encode({"op": "heartbeat"}))
                    await asyncio.sleep(5)
                except BrokenPipeError:
                    self._log.warning("Heartbeat failed, reconnecting")
                    await self._reconnect()
        except asyncio.CancelledError:
            self._log.debug("Canceled task 'post_connect_heartbeat'")
            return

    async def _keep_alive(self) -> None:
        self._log.info(f"Starting keep-alive every {self.keep_alive_period}s")
        while True:
            try:
                await asyncio.sleep(self.keep_alive_period)
                self._log.info("Sending keep-alive")
                await self._client.keep_alive()
            except asyncio.CancelledError:
                self._log.debug("Canceled task 'keep_alive'")
                return

    async def _disconnect(self) -> None:
        # Cancel tasks
        if self._heartbeat_task:
            self._log.debug("Canceling task 'heartbeat'")
            self._heartbeat_task.cancel()
            self._heartbeat_task = None

        if self._keep_alive_task:
            self._log.debug("Canceling task 'keep_alive'")
            self._keep_alive_task.cancel()
            self._keep_alive_task = None

        self._log.info("Closing streaming socket")
        await self._stream.disconnect()

        self._log.info("Closing BetfairClient")
        await self._client.disconnect()

    async def _reconnect(self) -> None:
        self._log.info("Attempting reconnect")
        if self._stream.is_connected:
            self._log.info("Stream connected: disconnecting")
            await self._stream.disconnect()
        await self._stream.connect()
        self._reconnect_in_progress = False

    def _reset(self) -> None:
        if self.is_connected:
            self._log.error("Cannot reset a connected data client")
            return

        self._subscribed_instrument_ids = set()

    def _dispose(self) -> None:
        if self.is_connected:
            self._log.error("Cannot dispose a connected data client")
            return

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    async def _subscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: int | None = None,
        params: dict[str, Any] | None = None,
    ) -> None:
        PyCondition.not_none(instrument_id, "instrument_id")

        instrument: BettingInstrument = self._instrument_provider.find(instrument_id)

        if instrument.market_id in self._subscribed_market_ids:
            self._log.warning(
                f"Already subscribed to market_id: {instrument.market_id} "
                f"[Instrument: {instrument_id.symbol}] <OrderBook> data",
            )
            return

        if self.subscription_status == SubscriptionStatus.SUBSCRIBED:
            self._log.debug("Already subscribed")
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
            f"Added market_id {instrument.market_id} for {instrument_id.symbol} <OrderBook> data",
        )

    async def delayed_subscribe(self, delay=0) -> None:
        self._log.debug(f"Scheduling subscribe for delay={delay}")
        await asyncio.sleep(delay)
        self._log.info(f"Sending subscribe for market_ids {self._subscribed_market_ids}")
        await self._stream.send_subscription_message(market_ids=list(self._subscribed_market_ids))
        self._log.info(f"Added market_ids {self._subscribed_market_ids} for <OrderBook> data")

    async def _subscribe_instrument(
        self,
        instrument_id: InstrumentId,
        params: dict[str, Any] | None = None,
    ) -> None:
        self._log.info("Skipping subscribe_instrument, Betfair subscribes as part of orderbook")

    async def _subscribe_quote_ticks(
        self,
        instrument_id: InstrumentId,
        params: dict[str, Any] | None = None,
    ) -> None:
        self._log.info("Skipping subscribe_quote_ticks, Betfair subscribes as part of orderbook")

    async def _subscribe_trade_ticks(
        self,
        instrument_id: InstrumentId,
        params: dict[str, Any] | None = None,
    ) -> None:
        self._log.info("Skipping subscribe_trade_ticks, Betfair subscribes as part of orderbook")

    async def _subscribe_instruments(self, params: dict[str, Any] | None = None) -> None:
        for instrument in self._instrument_provider.list_all():
            self._handle_data(instrument)

    async def _subscribe_instrument_status(
        self,
        instrument_id: InstrumentId,
        params: dict[str, Any] | None = None,
    ) -> None:
        pass  # Subscribed as part of orderbook

    async def _subscribe_instrument_close(
        self,
        instrument_id: InstrumentId,
        params: dict[str, Any] | None = None,
    ) -> None:
        pass  # Subscribed as part of orderbook

    async def _unsubscribe_order_book_snapshots(
        self,
        instrument_id: InstrumentId,
        params: dict[str, Any] | None = None,
    ) -> None:
        # TODO - this could be done by removing the market from self.__subscribed_market_ids and resending the
        #  subscription message - when we have a use case

        self._log.warning("Betfair does not support unsubscribing from instruments")

    async def _unsubscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        params: dict[str, Any] | None = None,
    ) -> None:
        # TODO - this could be done by removing the market from self.__subscribed_market_ids and resending the
        #  subscription message - when we have a use case
        self._log.warning("Betfair does not support unsubscribing from instruments")

    async def _unsubscribe_instrument(
        self,
        instrument_id: InstrumentId,
        params: dict[str, Any] | None = None,
    ) -> None:
        self._log.info("Skipping unsubscribe_instrument, not applicable for Betfair")

    async def _unsubscribe_quote_ticks(
        self,
        instrument_id: InstrumentId,
        params: dict[str, Any] | None = None,
    ) -> None:
        self._log.info("Skipping unsubscribe_quote_ticks, not applicable for Betfair")

    async def _unsubscribe_trade_ticks(
        self,
        instrument_id: InstrumentId,
        params: dict[str, Any] | None = None,
    ) -> None:
        self._log.info("Skipping unsubscribe_trade_ticks, not applicable for Betfair")

    # -- STREAMS ----------------------------------------------------------------------------------

    def on_market_update(self, raw: bytes) -> None:
        """
        Handle an update from the data stream socket.
        """
        self._log.debug(f"[RECV]: {raw.decode()}")
        update = stream_decode(raw)
        if isinstance(update, MCM):
            self._on_market_update(mcm=update)
        elif isinstance(update, Connection):
            pass
        elif isinstance(update, Status):
            self._handle_status_message(update=update)
        else:
            raise RuntimeError

    def _on_market_update(self, mcm: MCM) -> None:
        self._check_stream_unhealthy(update=mcm)
        updates = self._parser.parse(mcm=mcm)
        for data in updates:
            self._log.debug(f"{data=}")
            PyCondition.type(data, Data, "data")
            if isinstance(data, BettingInstrument):
                self._on_instrument(data)
            else:
                self._handle_data(data)

    def _on_instrument(self, instrument: BettingInstrument):
        cache_instrument = self._cache.instrument(instrument.id)
        if cache_instrument is None:
            self._handle_data(instrument)
            return

        # We've received an update to an existing instrument, update any fields that have changed
        instrument = merge_instrument_fields(cache_instrument, instrument, self._log)
        self._handle_data(instrument)

    def _check_stream_unhealthy(self, update: MCM) -> None:
        if update.stream_unreliable:
            self._log.warning("Stream unhealthy, waiting for recovery")
            self.degrade()
        if update.mc is not None:
            for mc in update.mc:
                if mc.con:
                    ms_delay = self._clock.timestamp_ms() - update.pt
                    self._log.warning(f"Conflated stream - data received is delayed ({ms_delay}ms)")

    def _handle_status_message(self, update: Status) -> None:
        if update.is_error and update.connection_closed:
            self._log.error(f"Betfair connection closed: {update.error_message}")
            if update.error_code == "MAX_CONNECTION_LIMIT_EXCEEDED":
                raise RuntimeError("No more connections available")
            elif update.error_code == "SUBSCRIPTION_LIMIT_EXCEEDED":
                raise RuntimeError("Subscription request limit exceeded")
            elif update.error_code == "INVALID_SESSION_INFORMATION":
                if self._reconnect_in_progress:
                    self._log.info("Reconnect already in progress")
                    return
                self._log.info("Invalid session information, reconnecting client")
                self._reconnect_in_progress = True
                self._stream.is_connected = False
                self._client.reset_headers()
                self._log.info("Reconnecting socket")
                self.create_task(self._reconnect())
            else:
                if self._reconnect_in_progress:
                    self._log.info("Reconnect already in progress")
                    return
                self._log.info("Unknown failure message, scheduling restart")
                self._reconnect_in_progress = True
                self.create_task(self._reconnect())
