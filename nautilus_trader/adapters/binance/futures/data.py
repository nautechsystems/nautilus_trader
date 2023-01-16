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

from nautilus_trader.adapters.binance.common.data import BinanceCommonDataClient
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceEnumParser
from nautilus_trader.adapters.binance.common.schemas.market import BinanceOrderBookMsg
from nautilus_trader.adapters.binance.futures.http.market import BinanceFuturesMarketHttpAPI
from nautilus_trader.adapters.binance.futures.schemas.market import BinanceFuturesMarkPriceMsg
from nautilus_trader.adapters.binance.futures.schemas.market import BinanceFuturesTradeMsg
from nautilus_trader.adapters.binance.futures.types import BinanceFuturesMarkPriceUpdate
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.model.data.base import DataType
from nautilus_trader.model.data.base import GenericData
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.orderbook.data import OrderBookData
from nautilus_trader.model.orderbook.data import OrderBookSnapshot
from nautilus_trader.msgbus.bus import MessageBus


class BinanceFuturesDataClient(BinanceCommonDataClient):
    """
    Provides a data client for the `Binance Futures` exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : BinanceHttpClient
        The binance HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    logger : Logger
        The logger for the client.
    instrument_provider : InstrumentProvider
        The instrument provider.
    account_type : BinanceAccountType
        The account type for the client.
    base_url_ws : str, optional
        The base URL for the WebSocket client.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: BinanceHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
        instrument_provider: InstrumentProvider,
        account_type: BinanceAccountType = BinanceAccountType.FUTURES_USDT,
        base_url_ws: Optional[str] = None,
    ):
        if not account_type.is_futures:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"`BinanceAccountType` not FUTURES_USDT or FUTURES_COIN, was {account_type}",  # pragma: no cover
            )

        market = BinanceFuturesMarketHttpAPI(client, account_type)
        super().__init__(
            loop=loop,
            client=client,
            market=market,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            instrument_provider=instrument_provider,
            account_type=account_type,
            enum_parser=BinanceEnumParser(),
            base_url_ws=base_url_ws,
        )

        # Register additional futures websocket handlers
        self._ws_handlers["@depth"] = self._handle_book_partial_update
        self._ws_handlers[
            "@trade"
        ] = self._handle_trade  # NOTE @trade is an undocumented endpoint for Futures exchanges
        self._ws_handlers["@markPrice"] = self._handle_mark_price
        # Websocket msgspec decoders
        self._decoder_futures_trade_msg = msgspec.json.Decoder(BinanceFuturesTradeMsg)
        self._decoder_futures_mark_price_msg = msgspec.json.Decoder(BinanceFuturesMarkPriceMsg)

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    async def _subscribe(self, data_type: DataType) -> None:
        if data_type.type == BinanceFuturesMarkPriceUpdate:
            if not self._binance_account_type.is_futures:
                self._log.error(
                    f"Cannot subscribe to `BinanceFuturesMarkPriceUpdate` "
                    f"for {self._binance_account_type.value} account types.",
                )
                return
            instrument_id: Optional[InstrumentId] = data_type.metadata.get("instrument_id")
            if instrument_id is None:
                self._log.error(
                    "Cannot subscribe to `BinanceFuturesMarkPriceUpdate` "
                    "no instrument ID in `data_type` metadata.",
                )
                return
            self._ws_client.subscribe_mark_price(instrument_id.symbol.value, speed=1000)
        else:
            self._log.error(
                f"Cannot subscribe to {data_type.type} (not implemented).",
            )

    async def _subscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Optional[int] = None,
        kwargs: Optional[dict] = None,
    ) -> None:
        update_speed = 0  # NOTE undocumented 0ms update speed for Futures
        if "update_speed" in kwargs:
            update_speed = kwargs["update_speed"]

        await self._subscribe_order_book(
            instrument_id=instrument_id,
            book_type=book_type,
            update_speed=update_speed,
            depth=depth,
        )

    async def _subscribe_order_book_snapshots(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Optional[int] = None,
        kwargs: Optional[dict] = None,
    ) -> None:
        update_speed = 0  # NOTE undocumented 0ms update speed for Futures
        if "update_speed" in kwargs:
            update_speed = kwargs["update_speed"]

        await self._subscribe_order_book(
            instrument_id=instrument_id,
            book_type=book_type,
            update_speed=update_speed,
            depth=depth,
        )

    async def _subscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        self._log.warning(
            "Trade ticks have been requested from a `Binance Futures` exchange. "
            "This functionality is not officially documented or supported.",
        )
        self._ws_client.subscribe_trades(instrument_id.symbol.value)

    async def _unsubscribe(self, data_type: DataType) -> None:
        if data_type.type == BinanceFuturesMarkPriceUpdate:
            if not self._binance_account_type.is_futures:
                self._log.error(
                    "Cannot unsubscribe from `BinanceFuturesMarkPriceUpdate` "
                    f"for {self._binance_account_type.value} account types.",
                )
                return
            instrument_id: Optional[InstrumentId] = data_type.metadata.get("instrument_id")
            if instrument_id is None:
                self._log.error(
                    "Cannot subscribe to `BinanceFuturesMarkPriceUpdate` no instrument ID in `data_type` metadata.",
                )
                return
        else:
            self._log.error(
                f"Cannot unsubscribe from {data_type.type} (not implemented).",
            )

    # -- REQUESTS ---------------------------------------------------------------------------------

    def _handle_book_partial_update(self, raw: bytes) -> None:
        msg: BinanceOrderBookMsg = msgspec.json.decode(raw, type=BinanceOrderBookMsg)
        instrument_id: InstrumentId = self._get_cached_instrument_id(msg.data.s)
        book_snapshot: OrderBookSnapshot = msg.data.parse_to_order_book_snapshot(
            instrument_id=instrument_id,
            ts_init=self._clock.timestamp_ns(),
        )
        # Check if book buffer active
        book_buffer: Optional[list[OrderBookData]] = self._book_buffer.get(instrument_id)
        if book_buffer is not None:
            book_buffer.append(book_snapshot)
        else:
            self._handle_data(book_snapshot)

    def _handle_trade(self, raw: bytes) -> None:
        msg: BinanceFuturesTradeMsg = msgspec.json.decode(raw, type=BinanceFuturesTradeMsg)
        instrument_id: InstrumentId = self._get_cached_instrument_id(msg.data.s)
        trade_tick: TradeTick = msg.data.parse_to_trade_tick(
            instrument_id=instrument_id,
            ts_init=self._clock.timestamp_ns(),
        )
        self._handle_data(trade_tick)

    def _handle_mark_price(self, raw: bytes) -> None:
        msg = self._decoder_futures_mark_price_msg.decode(raw)
        instrument_id: InstrumentId = self._get_cached_instrument_id(msg.data.s)
        data = msg.data.parse_to_binance_futures_mark_price_update(
            instrument_id=instrument_id,
            ts_init=self._clock.timestamp_ns(),
        )
        data_type = DataType(
            BinanceFuturesMarkPriceUpdate,
            metadata={"instrument_id": instrument_id},
        )
        generic = GenericData(data_type=data_type, data=data)
        self._handle_data(generic)
