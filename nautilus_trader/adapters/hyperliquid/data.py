# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from __future__ import annotations

import asyncio
from typing import Any

from nautilus_trader.adapters.hyperliquid.config import HyperliquidDataClientConfig
from nautilus_trader.adapters.hyperliquid.constants import HYPERLIQUID_VENUE
from nautilus_trader.adapters.hyperliquid.providers import HyperliquidInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.data import Data
from nautilus_trader.core.datetime import ensure_pydatetime_utc
from nautilus_trader.data.messages import RequestBars
from nautilus_trader.data.messages import RequestInstrument
from nautilus_trader.data.messages import RequestInstruments
from nautilus_trader.data.messages import RequestQuoteTicks
from nautilus_trader.data.messages import RequestTradeTicks
from nautilus_trader.data.messages import SubscribeBars
from nautilus_trader.data.messages import SubscribeData
from nautilus_trader.data.messages import SubscribeFundingRates
from nautilus_trader.data.messages import SubscribeIndexPrices
from nautilus_trader.data.messages import SubscribeInstrument
from nautilus_trader.data.messages import SubscribeInstruments
from nautilus_trader.data.messages import SubscribeMarkPrices
from nautilus_trader.data.messages import SubscribeOrderBook
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.data.messages import UnsubscribeBars
from nautilus_trader.data.messages import UnsubscribeData
from nautilus_trader.data.messages import UnsubscribeFundingRates
from nautilus_trader.data.messages import UnsubscribeIndexPrices
from nautilus_trader.data.messages import UnsubscribeInstrument
from nautilus_trader.data.messages import UnsubscribeInstruments
from nautilus_trader.data.messages import UnsubscribeMarkPrices
from nautilus_trader.data.messages import UnsubscribeOrderBook
from nautilus_trader.data.messages import UnsubscribeQuoteTicks
from nautilus_trader.data.messages import UnsubscribeTradeTicks
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import CustomData
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import FundingRateUpdate
from nautilus_trader.model.data import capsule_to_data
from nautilus_trader.model.identifiers import ClientId


_PYO3HyperliquidAllMids: Any = getattr(nautilus_pyo3, "HyperliquidAllMids", None)


class HyperliquidAllMids(Data):
    """
    Python data object for Hyperliquid allMids payload.

    Notes
    -----
    allMids uses coin -> instrument mapping during decoding. Ensure the
    instrument provider is configured with `load_all=True` (or sufficient
    `load_ids`) so incoming coins can be mapped to `InstrumentId`.

    """

    def __init__(self, mids: dict[str, str], ts_event: int, ts_init: int) -> None:
        self.mids = mids
        self._ts_event = ts_event
        self._ts_init = ts_init

    @property
    def ts_event(self) -> int:
        return self._ts_event

    @property
    def ts_init(self) -> int:
        return self._ts_init

    @staticmethod
    def from_pyo3(pyo3_all_mids: Any) -> HyperliquidAllMids:
        mids = {
            str(instrument_id): str(price) for instrument_id, price in pyo3_all_mids.mids.items()
        }
        return HyperliquidAllMids(
            mids=mids,
            ts_event=pyo3_all_mids.ts_event,
            ts_init=pyo3_all_mids.ts_init,
        )


class HyperliquidDataClient(LiveMarketDataClient):
    """
    Provides a data client for the Hyperliquid decentralized exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : nautilus_pyo3.HyperliquidHttpClient
        The Hyperliquid HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : HyperliquidInstrumentProvider
        The instrument provider.
    config : HyperliquidDataClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: nautilus_pyo3.HyperliquidHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: HyperliquidInstrumentProvider,
        config: HyperliquidDataClientConfig,
        name: str | None = None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or HYPERLIQUID_VENUE.value),
            venue=HYPERLIQUID_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )

        self._instrument_provider: HyperliquidInstrumentProvider = instrument_provider

        # Configuration
        self._config = config
        environment = config.environment or nautilus_pyo3.HyperliquidEnvironment.MAINNET
        self._log.info(f"config.environment={environment}", LogColor.BLUE)
        self._log.info(f"config.http_timeout_secs={config.http_timeout_secs}", LogColor.BLUE)
        self._log.info(f"{config.proxy_url=}", LogColor.BLUE)

        # HTTP client (uses EVM private key for authentication, not API key)
        self._http_client = client
        self._log.info("HTTP client initialized", LogColor.BLUE)

        # WebSocket client for market data
        self._ws_client = nautilus_pyo3.HyperliquidWebSocketClient(
            url=config.base_url_ws,
            environment=environment,
            proxy_url=config.proxy_url,
        )

    @property
    def instrument_provider(self) -> HyperliquidInstrumentProvider:
        return self._instrument_provider

    async def _connect(self) -> None:
        await self.instrument_provider.initialize()
        self._cache_instruments()
        self._send_all_instruments_to_data_engine()

        instruments = self.instrument_provider.instruments_pyo3()

        await self._ws_client.connect(self._loop, instruments, self._handle_msg)
        self._log.info(f"Connected to WebSocket {self._ws_client.url}", LogColor.BLUE)

    async def _disconnect(self) -> None:
        # Delay to allow websocket to send any unsubscribe messages
        await asyncio.sleep(1.0)

        if not self._ws_client.is_closed():
            self._log.info("Disconnecting WebSocket")
            await self._ws_client.close()
            self._log.info(
                f"Disconnected from WebSocket {self._ws_client.url}",
                LogColor.BLUE,
            )

    def _cache_instruments(self) -> None:
        # Ensures instrument definitions are available for correct
        # price and size precisions when parsing responses
        instruments_pyo3 = self.instrument_provider.instruments_pyo3()
        for inst in instruments_pyo3:
            self._http_client.cache_instrument(inst)

        self._log.debug("Cached instruments", LogColor.MAGENTA)

    def _send_all_instruments_to_data_engine(self) -> None:
        for instrument in self.instrument_provider.get_all().values():
            self._handle_data(instrument)

        for currency in self.instrument_provider.currencies().values():
            self._cache.add_currency(currency)

    def _handle_msg(self, msg: Any) -> None:
        try:
            if nautilus_pyo3.is_pycapsule(msg):
                # The capsule will fall out of scope at the end of this method,
                # and eventually be garbage collected. The contained pointer
                # to `Data` is still owned and managed by Rust.
                data = capsule_to_data(msg)
                self._handle_data(data)
            elif isinstance(msg, nautilus_pyo3.CustomData):
                if _PYO3HyperliquidAllMids is None:
                    self._log.warning("HyperliquidAllMids type is not available in nautilus_pyo3")
                    return

                if not isinstance(msg.data, _PYO3HyperliquidAllMids):
                    self._log.warning(
                        f"Unsupported Hyperliquid custom payload type: {type(msg.data).__name__}",
                    )
                    return

                inner = HyperliquidAllMids.from_pyo3(msg.data)
                data_type = DataType(HyperliquidAllMids, metadata=msg.data_type.metadata)
                self._handle_data(CustomData(data_type=data_type, data=inner))
            elif isinstance(msg, nautilus_pyo3.FundingRateUpdate):
                data = FundingRateUpdate.from_pyo3(msg)
                self._handle_data(data)
            else:
                self._log.warning(f"Cannot handle message {msg}: not implemented")
        except Exception as e:
            self._log.exception("Error handling websocket message", e)

    # -- SUBSCRIPTIONS ---------------------------------------------------------------------------------

    async def _subscribe(self, command: SubscribeData) -> None:
        data_type = command.data_type
        data_type_name = data_type.type.__name__

        if data_type_name == "HyperliquidAllMids":
            if not self.instrument_provider.get_all():
                self._log.warning(
                    "Subscribing to HyperliquidAllMids with an empty instrument mapping. "
                    "Set instrument_provider.load_all=True (or provide sufficient load_ids) "
                    "to decode allMids into InstrumentId-keyed data.",
                )

            metadata = data_type.metadata or {}
            dex_raw = metadata.get("dex")
            dex = str(dex_raw).strip() if dex_raw is not None else ""

            if dex:
                subscribe_all_mids_with_dex: Any = getattr(
                    self._ws_client,
                    "subscribe_all_mids_with_dex",
                    None,
                )

                if subscribe_all_mids_with_dex is None:
                    self._log.warning(
                        "Unsupported Hyperliquid allMids subscription: "
                        "WebSocket client does not expose subscribe_all_mids_with_dex",
                    )
                    return
                await subscribe_all_mids_with_dex(dex)
            else:
                subscribe_all_mids: Any = getattr(self._ws_client, "subscribe_all_mids", None)
                if subscribe_all_mids is None:
                    self._log.warning(
                        "Unsupported Hyperliquid allMids subscription: "
                        "WebSocket client does not expose subscribe_all_mids",
                    )
                    return
                await subscribe_all_mids()
            return

        self._log.warning(f"Unsupported custom data subscription: {data_type_name}")

    async def _subscribe_instrument(self, command: SubscribeInstrument) -> None:
        self._log.info(f"Subscribed to instrument updates for {command.instrument_id}")

    async def _subscribe_instruments(self, command: SubscribeInstruments) -> None:
        self._log.info("Subscribed to instruments updates")

    async def _subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_book(pyo3_instrument_id)

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_quotes(pyo3_instrument_id)

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_trades(pyo3_instrument_id)

    async def _subscribe_mark_prices(self, command: SubscribeMarkPrices) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_mark_prices(pyo3_instrument_id)

    async def _subscribe_index_prices(self, command: SubscribeIndexPrices) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_index_prices(pyo3_instrument_id)

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        pyo3_bar_type = nautilus_pyo3.BarType.from_str(str(command.bar_type))
        await self._ws_client.subscribe_bars(pyo3_bar_type)

    async def _subscribe_funding_rates(self, command: SubscribeFundingRates) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.subscribe_funding_rates(pyo3_instrument_id)

    async def _unsubscribe(self, command: UnsubscribeData) -> None:
        data_type = command.data_type
        data_type_name = data_type.type.__name__

        if data_type_name == "HyperliquidAllMids":
            metadata = data_type.metadata or {}
            dex_raw = metadata.get("dex")
            dex = str(dex_raw).strip() if dex_raw is not None else ""

            if dex:
                unsubscribe_all_mids_with_dex: Any = getattr(
                    self._ws_client,
                    "unsubscribe_all_mids_with_dex",
                    None,
                )

                if unsubscribe_all_mids_with_dex is None:
                    self._log.warning(
                        "Unsupported Hyperliquid allMids unsubscription: "
                        "WebSocket client does not expose unsubscribe_all_mids_with_dex",
                    )
                    return
                await unsubscribe_all_mids_with_dex(dex)
            else:
                unsubscribe_all_mids: Any = getattr(
                    self._ws_client,
                    "unsubscribe_all_mids",
                    None,
                )

                if unsubscribe_all_mids is None:
                    self._log.warning(
                        "Unsupported Hyperliquid allMids unsubscription: "
                        "WebSocket client does not expose unsubscribe_all_mids",
                    )
                    return
                await unsubscribe_all_mids()
            return

        self._log.warning(f"Unsupported custom data unsubscription: {data_type_name}")

    async def _unsubscribe_instrument(self, command: UnsubscribeInstrument) -> None:
        self._log.info(f"Unsubscribed from instrument updates for {command.instrument_id}")

    async def _unsubscribe_instruments(self, command: UnsubscribeInstruments) -> None:
        self._log.info("Unsubscribed from instruments updates")

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_book(pyo3_instrument_id)

    async def _unsubscribe_order_book(self, command: UnsubscribeOrderBook) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_book(pyo3_instrument_id)

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_quotes(pyo3_instrument_id)

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_trades(pyo3_instrument_id)

    async def _unsubscribe_mark_prices(self, command: UnsubscribeMarkPrices) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_mark_prices(pyo3_instrument_id)

    async def _unsubscribe_index_prices(self, command: UnsubscribeIndexPrices) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_index_prices(pyo3_instrument_id)

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        pyo3_bar_type = nautilus_pyo3.BarType.from_str(str(command.bar_type))
        await self._ws_client.unsubscribe_bars(pyo3_bar_type)

    async def _unsubscribe_funding_rates(self, command: UnsubscribeFundingRates) -> None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        await self._ws_client.unsubscribe_funding_rates(pyo3_instrument_id)

    # -- REQUESTS -----------------------------------------------------------------------------------

    async def _request_instrument(self, request: RequestInstrument) -> None:
        instrument = self.instrument_provider.find(request.instrument_id)
        if instrument:
            self._handle_data(instrument)
            self._log.debug(f"Sent instrument {request.instrument_id}")
        else:
            self._log.error(f"Instrument not found: {request.instrument_id}")

    async def _request_instruments(self, request: RequestInstruments) -> None:
        instruments = []

        for instrument_id in request.instrument_ids:
            instrument = self.instrument_provider.find(instrument_id)
            if instrument:
                instruments.append(instrument)
                self._handle_data(instrument)
                self._log.debug(f"Sent instrument {instrument_id}")
            else:
                self._log.warning(f"Instrument not found: {instrument_id}")

        if not instruments:
            self._log.warning("No instruments found for request")
        else:
            self._log.info(f"Sent {len(instruments)} instruments")

    async def _request_quote_ticks(self, request: RequestQuoteTicks) -> None:
        self._log.warning("Cannot request historical quotes: not supported by Hyperliquid")

    async def _request_trade_ticks(self, request: RequestTradeTicks) -> None:
        self._log.warning("Cannot request historical trades: not supported by Hyperliquid")

    async def _request_bars(self, request: RequestBars) -> None:
        pyo3_bar_type = nautilus_pyo3.BarType.from_str(str(request.bar_type))
        start = ensure_pydatetime_utc(request.start) if request.start else None
        end = ensure_pydatetime_utc(request.end) if request.end else None

        try:
            pyo3_bars = await self._http_client.request_bars(
                pyo3_bar_type,
                start,
                end,
                request.limit,
            )
            bars = Bar.from_pyo3_list(pyo3_bars)

            self._handle_bars(
                request.bar_type,
                bars,
                request.id,
                request.start,
                request.end,
                request.params,
            )
        except Exception as e:
            self._log.exception(f"Error requesting bars for {request.bar_type}", e)
