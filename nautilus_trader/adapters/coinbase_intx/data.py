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

from nautilus_trader.adapters.coinbase_intx.config import CoinbaseIntxDataClientConfig
from nautilus_trader.adapters.coinbase_intx.constants import COINBASE_INTX
from nautilus_trader.adapters.coinbase_intx.providers import CoinbaseIntxInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.data.messages import RequestBars
from nautilus_trader.data.messages import RequestInstrument
from nautilus_trader.data.messages import RequestInstruments
from nautilus_trader.data.messages import RequestQuoteTicks
from nautilus_trader.data.messages import RequestTradeTicks
from nautilus_trader.data.messages import SubscribeBars
from nautilus_trader.data.messages import SubscribeIndexPrices
from nautilus_trader.data.messages import SubscribeInstrument
from nautilus_trader.data.messages import SubscribeInstruments
from nautilus_trader.data.messages import SubscribeMarkPrices
from nautilus_trader.data.messages import SubscribeOrderBook
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.data.messages import UnsubscribeBars
from nautilus_trader.data.messages import UnsubscribeIndexPrices
from nautilus_trader.data.messages import UnsubscribeInstrument
from nautilus_trader.data.messages import UnsubscribeInstruments
from nautilus_trader.data.messages import UnsubscribeMarkPrices
from nautilus_trader.data.messages import UnsubscribeOrderBook
from nautilus_trader.data.messages import UnsubscribeQuoteTicks
from nautilus_trader.data.messages import UnsubscribeTradeTicks
from nautilus_trader.live.cancellation import DEFAULT_FUTURE_CANCELLATION_TIMEOUT
from nautilus_trader.live.cancellation import cancel_tasks_with_timeout
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import IndexPriceUpdate
from nautilus_trader.model.data import MarkPriceUpdate
from nautilus_trader.model.data import capsule_to_data
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.instruments import Instrument


class CoinbaseIntxDataClient(LiveMarketDataClient):
    """
    Provides a data client for the Coinbase International crypto exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : nautilus_pyo3.CoinbaseIntxHttpClient
        The Coinbase International HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : CoinbaseIntxInstrumentProvider
        The instrument provider.
    config : TardisDataClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: nautilus_pyo3.CoinbaseIntxHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: CoinbaseIntxInstrumentProvider,
        config: CoinbaseIntxDataClientConfig,
        name: str | None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or COINBASE_INTX),
            venue=None,  # Not applicable
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )
        self._instrument_provider: CoinbaseIntxInstrumentProvider = instrument_provider

        # Configuration
        self._config = config
        self._log.info(f"{config.http_timeout_secs=}", LogColor.BLUE)

        # HTTP API
        self._http_client = client
        self._log.info(f"REST API key {self._http_client.api_key}", LogColor.BLUE)

        # WebSocket API
        self._ws_client = nautilus_pyo3.CoinbaseIntxWebSocketClient(
            url=config.base_url_ws,
            api_key=config.api_key,
            api_secret=config.api_secret,
            api_passphrase=config.api_passphrase,
            heartbeat=20,
        )
        self._ws_client_futures: set[asyncio.Future] = set()

    @property
    def coinbase_intx_instrument_provider(self) -> CoinbaseIntxInstrumentProvider:
        return self._instrument_provider

    async def _connect(self) -> None:
        await self._instrument_provider.initialize()
        self._cache_instruments()
        self._send_all_instruments_to_data_engine()

        await self._ws_client.connect(
            instruments=self.coinbase_intx_instrument_provider.instruments_pyo3(),
            callback=self._handle_msg,
        )

        # Wait for connection to be established
        await self._ws_client.wait_until_active(timeout_secs=10.0)
        self._log.info(f"Connected to {self._ws_client.url}", LogColor.BLUE)
        self._log.info(f"WebSocket API key {self._ws_client.api_key}", LogColor.BLUE)
        self._log.info("Coinbase Intx API key authenticated", LogColor.GREEN)

        await self._ws_client.subscribe_instruments()

    async def _disconnect(self) -> None:
        # Delay to allow websocket to send any unsubscribe messages
        await asyncio.sleep(1.0)

        # Shutdown websockets
        if not self._ws_client.is_closed():
            self._log.info("Disconnecting websocket")

            await self._ws_client.close()

            self._log.info(f"Disconnected from {self._ws_client.url}", LogColor.BLUE)

        # Cancel any pending futures
        await cancel_tasks_with_timeout(
            self._ws_client_futures,
            self._log,
            timeout_secs=DEFAULT_FUTURE_CANCELLATION_TIMEOUT,
        )
        self._ws_client_futures.clear()

    def _cache_instruments(self) -> None:
        # Ensures instrument definitions are available for correct
        # price and size precisions when parsing responses.
        instruments_pyo3 = self.coinbase_intx_instrument_provider.instruments_pyo3()
        for inst in instruments_pyo3:
            self._http_client.add_instrument(inst)

        self._log.debug("Cached instruments", LogColor.MAGENTA)

    def _cache_instrument(self, instrument: Instrument) -> None:
        self._instrument_provider.add(instrument)
        self._http_client.add_instrument(instrument)

        self._log.debug(f"Cached instrument {instrument.id}", LogColor.MAGENTA)

    def _send_all_instruments_to_data_engine(self) -> None:
        for currency in self._instrument_provider.currencies().values():
            self._cache.add_currency(currency)

        for instrument in self._instrument_provider.get_all().values():
            self._handle_data(instrument)

    async def _subscribe_instruments(self, command: SubscribeInstruments) -> None:
        pass  # Do nothing further (subscribed automatically on start)

    async def _subscribe_instrument(self, command: SubscribeInstrument) -> None:
        pass  # Do nothing further (subscribed automatically on start)

    async def _subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        if command.book_type == BookType.L3_MBO:
            self._log.error(
                "Cannot subscribe to order book deltas: "
                "L3_MBO data is not published by Tardis. "
                "Valid book types are L1_MBP, L2_MBP",
            )
            return

        depth = 20 if not command.depth else command.depth

        if depth != 20:
            self._log.error(
                f"Cannot subscribe to order book deltas for other than depth 20, depth was {command.depth}",
            )
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_book([pyo3_instrument_id])

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_quotes([pyo3_instrument_id])

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_trades([pyo3_instrument_id])

    async def _subscribe_mark_prices(self, command: SubscribeMarkPrices) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_mark_prices([pyo3_instrument_id])

    async def _subscribe_index_prices(self, command: SubscribeIndexPrices) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_index_prices([pyo3_instrument_id])

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        pyo3_bar_type = nautilus_pyo3.BarType.from_str(str(command.bar_type))
        await self._ws_client.subscribe_bars(pyo3_bar_type)

    async def _unsubscribe_instruments(self, command: UnsubscribeInstruments) -> None:
        pass  # Do nothing further (subscriptions must be maintained for the clients)

    async def _unsubscribe_instrument(self, command: UnsubscribeInstrument) -> None:
        pass  # Do nothing further (subscriptions must be maintained for the clients)

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_book([pyo3_instrument_id])

    async def _unsubscribe_order_book_snapshots(self, command: UnsubscribeOrderBook) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_book([pyo3_instrument_id])

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_quotes([pyo3_instrument_id])

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_trades([pyo3_instrument_id])

    async def _unsubscribe_mark_prices(self, command: UnsubscribeMarkPrices) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_mark_prices([pyo3_instrument_id])

    async def _unsubscribe_index_prices(self, command: UnsubscribeIndexPrices) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_index_prices([pyo3_instrument_id])

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        pyo3_bar_type = nautilus_pyo3.BarType.from_str(str(command.bar_type))
        await self._ws_client.unsubscribe_bars(pyo3_bar_type)

    async def _request_instrument(self, request: RequestInstrument) -> None:
        if request.start is not None:
            self._log.warning(
                f"Requesting instrument {request.instrument_id} with specified `start` which has no effect",
            )

        if request.end is not None:
            self._log.warning(
                f"Requesting instrument {request.instrument_id} with specified `end` which has no effect",
            )

        instrument: Instrument | None = self._instrument_provider.find(request.instrument_id)
        if instrument is None:
            self._log.error(f"Cannot find instrument for {request.instrument_id}")
            return

        self._handle_instrument(instrument, request.id, request.start, request.end, request.params)

    async def _request_instruments(self, request: RequestInstruments) -> None:
        if request.start is not None:
            self._log.warning(
                f"Requesting instruments for {request.venue} with specified `start` which has no effect",
            )

        if request.end is not None:
            self._log.warning(
                f"Requesting instruments for {request.venue} with specified `end` which has no effect",
            )

        instruments = self._instrument_provider.get_all()

        self._handle_instruments(
            request.venue,
            instruments,
            request.id,
            request.start,
            request.end,
            request.params,
        )

    async def _request_quote_ticks(self, request: RequestQuoteTicks) -> None:
        self._log.error(
            f"Cannot request historical quotes for {request.instrument_id}: not supported by Coinbase International",
        )

    async def _request_trade_ticks(self, request: RequestTradeTicks) -> None:
        self._log.error(
            f"Cannot request historical trades for {request.instrument_id}: not supported by Coinbase International",
        )

    async def _request_bars(self, request: RequestBars) -> None:
        if request.bar_type.is_internally_aggregated():
            self._log.error(
                f"Cannot request {request.bar_type} bars: "
                f"only historical bars with EXTERNAL aggregation available from Coinbase International",
            )
            return

        if request.bar_type.spec.price_type != PriceType.LAST:
            self._log.error(
                f"Cannot request {request.bar_type} bars: "
                f"only historical bars for LAST price type available through Coinbase International",
            )
            return

        instrument = self._cache.instrument(request.bar_type.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot request bars: no instrument for {request.bar_type.instrument_id}",
            )
            return

        self._log.error(
            f"Cannot request historical bars for {request.bar_type}: not supported in this version",
        )

    def _handle_msg(self, msg: Any) -> None:
        try:
            if nautilus_pyo3.is_pycapsule(msg):
                # The capsule will fall out of scope at the end of this method,
                # and eventually be garbage collected. The contained pointer
                # to `Data` is still owned and managed by Rust.
                data = capsule_to_data(msg)
            elif isinstance(msg, nautilus_pyo3.MarkPriceUpdate):
                data = MarkPriceUpdate.from_pyo3(msg)
            elif isinstance(msg, nautilus_pyo3.IndexPriceUpdate):
                data = IndexPriceUpdate.from_pyo3(msg)
            elif isinstance(msg, nautilus_pyo3.CryptoPerpetual):
                self._cache_instrument(msg)
                data = CryptoPerpetual.from_pyo3(msg)
            elif isinstance(msg, nautilus_pyo3.CurrencyPair):
                self._cache_instrument(msg)
                data = CurrencyPair.from_pyo3(msg)
            else:
                self._log.error(f"Cannot handle message {msg}, not implemented")
                return

            self._handle_data(data)
        except Exception as e:
            self._log.exception("Error handling websocket message", e)
