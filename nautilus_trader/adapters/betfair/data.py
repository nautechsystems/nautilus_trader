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

from betfair_parser.spec.streaming import MCM
from betfair_parser.spec.streaming import Connection
from betfair_parser.spec.streaming import Status
from betfair_parser.spec.streaming import StatusErrorCode
from betfair_parser.spec.streaming import stream_decode

from nautilus_trader.adapters.betfair.client import BetfairHttpClient
from nautilus_trader.adapters.betfair.config import BetfairDataClientConfig
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
from nautilus_trader.data.messages import SubscribeInstrument
from nautilus_trader.data.messages import SubscribeInstrumentClose
from nautilus_trader.data.messages import SubscribeInstruments
from nautilus_trader.data.messages import SubscribeInstrumentStatus
from nautilus_trader.data.messages import SubscribeOrderBook
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.data.messages import UnsubscribeBars
from nautilus_trader.data.messages import UnsubscribeInstrument
from nautilus_trader.data.messages import UnsubscribeInstrumentClose
from nautilus_trader.data.messages import UnsubscribeInstruments
from nautilus_trader.data.messages import UnsubscribeInstrumentStatus
from nautilus_trader.data.messages import UnsubscribeOrderBook
from nautilus_trader.data.messages import UnsubscribeQuoteTicks
from nautilus_trader.data.messages import UnsubscribeTradeTicks
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments.betting import BettingInstrument


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
    config : BetfairDataClientConfig
        The configuration for the client.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: BetfairHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: BetfairInstrumentProvider,
        config: BetfairDataClientConfig,
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
        self.config = config
        self._log.info(f"{config.account_currency=}", LogColor.BLUE)
        self._log.info(f"{config.subscription_delay_secs=}", LogColor.BLUE)
        self._log.info(f"{config.keep_alive_secs=}", LogColor.BLUE)
        self._log.info(f"{config.stream_conflate_ms=}", LogColor.BLUE)

        # Clients
        self._client: BetfairHttpClient = client
        self._stream = BetfairMarketStreamClient(
            http_client=self._client,
            message_handler=self.on_market_update,
            certs_dir=config.certs_dir,
        )
        self._is_reconnecting = (
            False  # Necessary for coordination, as the clients rely on each other
        )

        self._parser = BetfairParser(currency=config.account_currency)

        # Async tasks
        self._keep_alive_task: asyncio.Task | None = None

        # Subscriptions
        self._subscription_status = SubscriptionStatus.UNSUBSCRIBED
        self._subscribed_instrument_ids: set[InstrumentId] = set()
        self._subscribed_market_ids: set[InstrumentId] = set()

    @property
    def instrument_provider(self) -> BetfairInstrumentProvider:
        """
        Return the instrument provider for the client.

        Returns
        -------
        BetfairInstrumentProvider

        """
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

        if not self._keep_alive_task:
            self._keep_alive_task = self.create_task(self._keep_alive())

        await self.stream_subscribe()

    async def stream_subscribe(self):
        # Subscribe per instrument provider config
        await self._stream.send_subscription_message(
            market_ids=self.instrument_provider.config.market_ids,
            event_type_ids=self.instrument_provider.config.event_type_ids,
            country_codes=self.instrument_provider.config.country_codes,
            market_types=self.instrument_provider.config.market_types,
            conflate_ms=self.config.stream_conflate_ms,
        )

    async def _keep_alive(self) -> None:
        keep_alive_hrs = self.config.keep_alive_secs / (60 * 60)
        self._log.info(f"Starting keep-alive every {keep_alive_hrs}hrs")
        while True:
            try:
                await asyncio.sleep(self.config.keep_alive_secs)
                self._log.info("Sending keep-alive")
                await self._client.keep_alive()
            except asyncio.CancelledError:
                self._log.debug("Canceled task 'keep_alive'")
                return

    async def _reconnect(self) -> None:
        self._log.info("Reconnecting to Betfair")
        self._is_reconnecting = True
        await self._client.reconnect()
        await self._stream.reconnect()
        await self.stream_subscribe()
        self._is_reconnecting = False

    async def _disconnect(self) -> None:
        # Cancel tasks
        if self._keep_alive_task:
            self._log.debug("Canceling task 'keep_alive'")
            self._keep_alive_task.cancel()
            self._keep_alive_task = None

        self._log.info("Closing streaming socket")
        await self._stream.disconnect()

        self._log.info("Closing BetfairClient")
        await self._client.disconnect()

    def _reset(self) -> None:
        if self._stream.is_active():
            self._log.error("Cannot reset a connected data client")
            return

        self._subscribed_instrument_ids = set()

    def _dispose(self) -> None:
        if self._stream.is_active():
            self._log.error("Cannot dispose a connected data client")
            return

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------
    async def _subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        self._log.info("Skipping subscribe_order_book_deltas, Betfair subscribes automatically")

    async def _subscribe_instrument(self, command: SubscribeInstrument) -> None:
        self._log.info("Skipping subscribe_instrument, Betfair subscribes automatically")

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        self._log.info("Skipping subscribe_quote_ticks, Betfair subscribes automatically")

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        self._log.info("Skipping subscribe_trade_ticks, Betfair subscribes automatically")

    async def _subscribe_instruments(self, command: SubscribeInstruments) -> None:
        for instrument in self._instrument_provider.list_all():
            self._handle_data(instrument)

    async def _subscribe_instrument_status(self, command: SubscribeInstrumentStatus) -> None:
        pass  # Subscribed as part of orderbook

    async def _subscribe_instrument_close(self, command: SubscribeInstrumentClose) -> None:
        pass  # Subscribed as part of orderbook

    async def _unsubscribe_order_book_snapshots(self, command: UnsubscribeOrderBook) -> None:
        # TODO - this could be done by removing the market from self.__subscribed_market_ids and resending the
        #  subscription message - when we have a use case
        self._log.warning("Betfair does not support unsubscribing")

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        # TODO - this could be done by removing the market from self.__subscribed_market_ids and resending the
        #  subscription message - when we have a use case
        self._log.warning("Betfair does not support unsubscribing")

    async def _unsubscribe_instrument(self, command: UnsubscribeInstrument) -> None:
        self._log.info("Skipping unsubscribe_instrument, not applicable for Betfair")

    async def _unsubscribe_instruments(self, command: UnsubscribeInstruments) -> None:
        self._log.info("Skipping unsubscribe_instrument, not applicable for Betfair")

    async def _unsubscribe_instrument_status(self, command: UnsubscribeInstrumentStatus) -> None:
        self._log.info("Skipping unsubscribe_instrument_status, not applicable for Betfair")

    async def _unsubscribe_instrument_close(self, command: UnsubscribeInstrumentClose) -> None:
        self._log.info("Skipping unsubscribe_instrument_status, not applicable for Betfair")

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        self._log.info("Skipping unsubscribe_quote_ticks, not applicable for Betfair")

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        self._log.info("Skipping unsubscribe_trade_ticks, not applicable for Betfair")

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        self._log.info("Skipping unsubscribe_bars, not applicable for Betfair")

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
            self._log.warning("Stream unhealthy; pausing for recovery")
            self.degrade()
        if update.mc is not None:
            for mc in update.mc:
                if mc.con:
                    latency_ms = self._clock.timestamp_ms() - update.pt
                    self._log.warning(f"Stream conflation detected: latency ~{latency_ms}ms")

    def _handle_status_message(self, update: Status) -> None:
        if update.is_error:
            if update.error_code == StatusErrorCode.MAX_CONNECTION_LIMIT_EXCEEDED:
                raise RuntimeError("No more connections available")
            elif update.error_code == StatusErrorCode.SUBSCRIPTION_LIMIT_EXCEEDED:
                raise RuntimeError("Subscription request limit exceeded")

            self._log.warning(f"Betfair API error: {update.error_message}")

            if update.connection_closed:
                self._log.warning("Betfair connection closed")
                if self._is_reconnecting:
                    self._log.info("Reconnect already in progress")
                    return
                self.create_task(self._reconnect())
