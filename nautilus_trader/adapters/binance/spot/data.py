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
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.spot.http.market import BinanceSpotMarketHttpAPI
from nautilus_trader.adapters.binance.spot.parsing.data import parse_spot_book_snapshot
from nautilus_trader.adapters.binance.spot.parsing.data import parse_spot_trade_tick_ws
from nautilus_trader.adapters.binance.spot.schemas.market import BinanceSpotOrderBookPartialDepthMsg
from nautilus_trader.adapters.binance.spot.schemas.market import BinanceSpotTradeMsg
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.orderbook.data import OrderBookData
from nautilus_trader.model.orderbook.data import OrderBookSnapshot
from nautilus_trader.msgbus.bus import MessageBus


class BinanceSpotDataClient(BinanceCommonDataClient):
    """
    Provides a data client for the `Binance Spot/Margin` exchange.

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
        account_type: BinanceAccountType = BinanceAccountType.SPOT,
        base_url_ws: Optional[str] = None,
    ):
        super().__init__(
            loop=loop,
            client=client,
            market=BinanceSpotMarketHttpAPI(
                account_type=account_type,
                client=client,
            ),
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            instrument_provider=instrument_provider,
            account_type=account_type,
            base_url_ws=base_url_ws,
        )

        if account_type not in (BinanceAccountType.SPOT, BinanceAccountType.MARGIN):
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"`BinanceAccountType` not SPOT or MARGIN, was {account_type}",  # pragma: no cover
            )

        # Register additional spot/margin websocket handlers
        futures_ws_handlers = {
            "@depth": self._handle_book_partial_update,
            "@trade": self._handle_trade,
        }
        self._ws_handlers.update(futures_ws_handlers)

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    async def _subscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Optional[int] = None,
        kwargs: Optional[dict] = None,
    ) -> None:
        update_speed = 100
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
        update_speed = 100
        if "update_speed" in kwargs:
            update_speed = kwargs["update_speed"]

        await self._subscribe_order_book(
            instrument_id=instrument_id,
            book_type=book_type,
            update_speed=update_speed,
            depth=depth,
        )

    async def _subscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        self._ws_client.subscribe_trades(instrument_id.symbol.value)

    # -- REQUESTS ---------------------------------------------------------------------------------

    def _handle_book_partial_update(self, raw: bytes) -> None:
        msg: BinanceSpotOrderBookPartialDepthMsg = msgspec.json.decode(
            raw,
            type=BinanceSpotOrderBookPartialDepthMsg,
        )
        instrument_id: InstrumentId = self._get_cached_instrument_id(
            msg.stream.partition("@")[0].upper(),
        )
        book_snapshot: OrderBookSnapshot = parse_spot_book_snapshot(
            instrument_id=instrument_id,
            data=msg.data,
            ts_init=self._clock.timestamp_ns(),
        )
        # Check if book buffer active
        book_buffer: Optional[list[OrderBookData]] = self._book_buffer.get(instrument_id)
        if book_buffer is not None:
            book_buffer.append(book_snapshot)
        else:
            self._handle_data(book_snapshot)

    def _handle_trade(self, raw: bytes) -> None:
        msg: BinanceSpotTradeMsg = msgspec.json.decode(raw, type=BinanceSpotTradeMsg)
        instrument_id: InstrumentId = self._get_cached_instrument_id(msg.data.s)
        trade_tick: TradeTick = parse_spot_trade_tick_ws(
            instrument_id=instrument_id,
            data=msg.data,
            ts_init=self._clock.timestamp_ns(),
        )
        self._handle_data(trade_tick)
