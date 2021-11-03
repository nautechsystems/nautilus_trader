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
from typing import Dict, List, Optional

import orjson
import pandas as pd

from nautilus_trader.adapters.binance.common import BINANCE_VENUE
from nautilus_trader.adapters.binance.data_types import BinanceBar
from nautilus_trader.adapters.binance.http.api.spot_market import BinanceSpotMarketHttpAPI
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.error import BinanceError
from nautilus_trader.adapters.binance.parsing import parse_bar
from nautilus_trader.adapters.binance.parsing import parse_bar_ws
from nautilus_trader.adapters.binance.parsing import parse_diff_depth_stream_ws
from nautilus_trader.adapters.binance.parsing import parse_quote_tick_ws
from nautilus_trader.adapters.binance.parsing import parse_ticker_ws
from nautilus_trader.adapters.binance.parsing import parse_trade_tick
from nautilus_trader.adapters.binance.parsing import parse_trade_tick_ws
from nautilus_trader.adapters.binance.providers import BinanceInstrumentProvider
from nautilus_trader.adapters.binance.websocket.spot import BinanceSpotWebSocket
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.c_enums.bar_aggregation import BarAggregationParser
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.msgbus.bus import MessageBus


class BinanceDataClient(LiveMarketDataClient):
    """
    Provides a data client for the Binance exchange.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: BinanceHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
        instrument_provider: BinanceInstrumentProvider,
    ):
        """
        Initialize a new instance of the ``BinanceDataClient`` class.

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
        instrument_provider : BinanceInstrumentProvider
            The instrument provider.

        """
        super().__init__(
            loop=loop,
            client_id=ClientId(BINANCE_VENUE.value),
            instrument_provider=instrument_provider,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )

        self._client = client

        self._update_instrument_interval: int = 60 * 60  # Once per hour (hardcode)
        self._update_instruments_task: Optional[asyncio.Task] = None
        self._spot = BinanceSpotMarketHttpAPI(client=self._client)
        self._ws_spot = BinanceSpotWebSocket(
            loop=loop,
            clock=clock,
            logger=logger,
            handler=self._handle_spot_ws_message,
        )

    def connect(self):
        """
        Connect the client to Binance.
        """
        self._log.info("Connecting...")
        self._loop.create_task(self._connect())

    def disconnect(self):
        """
        Disconnect the client from Binance.
        """
        self._log.info("Disconnecting...")
        self._loop.create_task(self._disconnect())

    async def _connect(self):
        if not self._client.connected:
            await self._client.connect()
        try:
            await self._instrument_provider.load_all_or_wait_async()
        except BinanceError as ex:
            self._log.ex(ex)
            return

        self._send_all_instruments_to_data_engine()
        self._schedule_subscribed_instruments_update(self._update_instrument_interval)

        self._loop.create_task(self._connect_websockets())

        self._set_connected(True)
        self._log.info("Connected.")

    async def _connect_websockets(self):
        self._log.info("Awaiting subscriptions...")
        await asyncio.sleep(2)
        if self._ws_spot.has_subscriptions:
            await self._ws_spot.connect()

    async def _disconnect(self):
        if self._update_instruments_task:
            self._log.debug("Canceling update instruments task...")
            self._update_instruments_task.cancel()

        if self._ws_spot.is_connected:
            self._log.debug("Disconnecting websockets...")
            self._ws_spot.disconnect()

        if self._client.connected:
            await self._client.disconnect()

        self._set_connected(False)
        self._log.info("Disconnected.")

    # -- SUBSCRIPTIONS -----------------------------------------------------------------------------

    def subscribe_instruments(self):
        """
        Subscribe to instrument data for the venue.

        """
        for instrument_id in list(self._instrument_provider.get_all().keys()):
            self._add_subscription_instrument(instrument_id)

    def subscribe_instrument(self, instrument_id: InstrumentId):
        """
        Subscribe to instrument data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to subscribe to.

        """
        self._add_subscription_instrument(instrument_id)

    def subscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        kwargs: dict = None,
    ):
        if book_type == BookType.L3_MBO:
            self._log.error(
                "Cannot subscribe to order book deltas: "
                "L3_MBO data is not published by Binance. "
                "Valid book types are L1_TBBO, L2_MBP."
            )
            return

        self._ws_spot.subscribe_diff_book_depth(instrument_id.symbol.value, speed=100)

    def subscribe_order_book_snapshots(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: int = 0,
        kwargs: dict = None,
    ):
        if book_type == BookType.L3_MBO:
            self._log.error(
                "Cannot subscribe to order book deltas: "
                "L3_MBO data is not published by Binance. "
                "Valid book types are L1_TBBO, L2_MBP."
            )
            return

        if depth == 0:
            # Convert to maximum depth
            depth = 20

        if depth not in (5, 10, 20):
            self._log.error(
                "Cannot subscribe to order book snapshots: "
                f"invalid depth, was {depth}. "
                "Valid depths are 0 (max), 5, 10, 20.",
            )
            return

        self._ws_spot.subscribe_partial_book_depth(
            symbol=instrument_id.symbol.value,
            depth=depth,
            speed=100,
        )

    def subscribe_ticker(self, instrument_id: InstrumentId):
        self._ws_spot.subscribe_ticker(instrument_id.symbol.value)

    def subscribe_quote_ticks(self, instrument_id: InstrumentId):
        self._ws_spot.subscribe_book_ticker(instrument_id.symbol.value)

    def subscribe_trade_ticks(self, instrument_id: InstrumentId):
        self._ws_spot.subscribe_trades(instrument_id.symbol.value)

    def subscribe_bars(self, bar_type: BarType):
        PyCondition.true(bar_type.is_externally_aggregated(), "aggregation_source is not EXTERNAL")

        if not bar_type.spec.is_time_aggregated():
            self._log.error(
                f"Cannot subscribe to {bar_type}: only time bars are aggregated by Binance."
            )
            return

        if bar_type.spec.aggregation == BarAggregation.SECOND:
            self._log.error(
                f"Cannot subscribe to {bar_type}: second bars are not aggregated by Binance."
            )
            return

        if bar_type.spec.aggregation == BarAggregation.MINUTE:
            resolution = "m"
        elif bar_type.spec.aggregation == BarAggregation.HOUR:
            resolution = "h"
        elif bar_type.spec.aggregation == BarAggregation.DAY:
            resolution = "d"
        else:  # pragma: no cover (design-time error)
            raise RuntimeError(
                f"invalid aggregation period, "
                f"was {BarAggregationParser.from_str(bar_type.spec.aggregation)}",
            )

        self._ws_spot.subscribe_bars(
            symbol=bar_type.instrument_id.symbol.value, interval=f"{bar_type.spec.step}{resolution}"
        )

    def subscribe_instrument_status_updates(self, instrument_id: InstrumentId):
        self._log.warning(
            "Cannot subscribe to instrument status updates: "
            "Not currently supported for the Binance integration.",
        )

    def subscribe_instrument_close_prices(self, instrument_id: InstrumentId):
        self._log.warning(
            "Cannot subscribe to instrument status updates: "
            "Not currently supported for the Binance integration.",
        )

    def unsubscribe_instruments(self):
        """
        Unsubscribe from instrument data for the venue.

        """
        for instrument_id in list(self._instrument_provider.get_all().keys()):
            self._remove_subscription_instrument(instrument_id)

    def unsubscribe_instrument(self, instrument_id: InstrumentId):
        """
        Unsubscribe from instrument data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to unsubscribe from.

        """
        self._remove_subscription_instrument(instrument_id)

    def unsubscribe_order_book_deltas(self, instrument_id: InstrumentId):
        self._remove_subscription_order_book_deltas(instrument_id)  # pragma: no cover

    def unsubscribe_order_book_snapshots(self, instrument_id: InstrumentId):
        self._remove_subscription_order_book_snapshots(instrument_id)  # pragma: no cover

    def unsubscribe_ticker(self, instrument_id: InstrumentId):
        self._remove_subscription_ticker(instrument_id)  # pragma: no cover

    def unsubscribe_quote_ticks(self, instrument_id: InstrumentId):
        self._remove_subscription_quote_ticks(instrument_id)  # pragma: no cover

    def unsubscribe_trade_ticks(self, instrument_id: InstrumentId):
        self._remove_subscription_trade_ticks(instrument_id)  # pragma: no cover

    def unsubscribe_bars(self, bar_type: BarType):
        self._remove_subscription_bars(bar_type)  # pragma: no cover

    def unsubscribe_instrument_status_updates(self, instrument_id: InstrumentId):
        self._remove_subscription_instrument_status_updates(instrument_id)  # pragma: no cover

    def unsubscribe_instrument_close_prices(self, instrument_id: InstrumentId):
        self._remove_subscription_instrument_close_prices(instrument_id)  # pragma: no cover

    # -- REQUESTS ----------------------------------------------------------------------------------

    def request_quote_ticks(
        self,
        instrument_id: InstrumentId,
        from_datetime: pd.Timestamp,
        to_datetime: pd.Timestamp,
        limit: int,
        correlation_id: UUID4,
    ):
        self._log.error(
            "Cannot request historical quote ticks: " "not published by Binance.",
        )

    def request_trade_ticks(
        self,
        instrument_id: InstrumentId,
        from_datetime: pd.Timestamp,
        to_datetime: pd.Timestamp,
        limit: int,
        correlation_id: UUID4,
    ):
        if limit == 0 or limit > 1000:
            limit = 1000

        if from_datetime is not None or to_datetime is not None:
            self._log.warning(
                "Trade ticks have been requested with a from/to time range, "
                f"however the request will be for the most recent {limit}."
            )

        self._loop.create_task(self._request_trade_ticks(instrument_id, limit, correlation_id))

    async def _request_trade_ticks(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
    ):
        data: bytes = await self._spot.trades(instrument_id.symbol.value, limit)
        response: List = orjson.loads(data)

        ticks: List[TradeTick] = [
            parse_trade_tick(
                msg=t,
                instrument_id=instrument_id,
                ts_init=self._clock.timestamp_ns(),
            )
            for t in response
        ]

        self._handle_trade_ticks(instrument_id, ticks, correlation_id)

    def request_bars(
        self,
        bar_type: BarType,
        from_datetime: pd.Timestamp,
        to_datetime: pd.Timestamp,
        limit: int,
        correlation_id: UUID4,
    ):
        if bar_type.is_internally_aggregated():
            self._log.error(
                f"Cannot request {bar_type}: "
                f"only historical bars with EXTERNAL aggregation available from Binance."
            )
            return

        if not bar_type.spec.is_time_aggregated():
            self._log.error(
                f"Cannot request {bar_type}: " f"only time bars are aggregated by Binance."
            )
            return

        if bar_type.spec.aggregation == BarAggregation.SECOND:
            self._log.error(
                f"Cannot request {bar_type}: " f"second bars are not aggregated by Binance."
            )
            return

        if bar_type.spec.price_type != PriceType.LAST:
            self._log.error(
                f"Cannot request {bar_type}: "
                f"only historical bars for LAST price type available from Binance."
            )
            return

        self._loop.create_task(
            self._request_bars(
                bar_type=bar_type,
                from_datetime=from_datetime,
                to_datetime=to_datetime,
                limit=limit,
                correlation_id=correlation_id,
            )
        )

    async def _request_bars(
        self,
        bar_type: BarType,
        from_datetime: pd.Timestamp,
        to_datetime: pd.Timestamp,
        limit: int,
        correlation_id: UUID4,
    ):
        if limit == 0 or limit > 1000:
            limit = 1000

        if bar_type.spec.aggregation == BarAggregation.MINUTE:
            resolution = "m"
        elif bar_type.spec.aggregation == BarAggregation.HOUR:
            resolution = "h"
        elif bar_type.spec.aggregation == BarAggregation.DAY:
            resolution = "d"
        else:  # pragma: no cover (design-time error)
            raise RuntimeError(
                f"invalid aggregation period, "
                f"was {BarAggregationParser.from_str(bar_type.spec.aggregation)}",
            )

        start_time_ms = from_datetime.to_datetime64() * 1000 if from_datetime is not None else None
        end_time_ms = to_datetime.to_datetime64() * 1000 if to_datetime is not None else None

        data: bytes = await self._spot.klines(
            symbol=bar_type.instrument_id.symbol.value,
            interval=f"{bar_type.spec.step}{resolution}",
            start_time_ms=start_time_ms,
            end_time_ms=end_time_ms,
            limit=limit,
        )

        response: List = orjson.loads(data)

        bars: List[BinanceBar] = [
            parse_bar(bar_type, values=b, ts_init=self._clock.timestamp_ns()) for b in response
        ]
        partial: BinanceBar = bars.pop()

        self._handle_bars(bar_type, bars, partial, correlation_id)

    async def _subscribed_instruments_update(self, delay):
        await self._instrument_provider.load_all_async()

        self._send_all_instruments_to_data_engine()

        update = self.run_after_delay(delay, self._subscribed_instruments_update(delay))
        self._update_instruments_task = self._loop.create_task(update)

    def _send_all_instruments_to_data_engine(self):
        for instrument in self._instrument_provider.get_all().values():
            self._handle_data(instrument)

        for currency in self._instrument_provider.currencies().values():
            self._cache.add_currency(currency)

    def _schedule_subscribed_instruments_update(self, delay: int):
        update = self.run_after_delay(delay, self._subscribed_instruments_update(delay))
        self._update_instruments_task = self._loop.create_task(update)

    def _handle_spot_ws_message(self, raw: bytes):
        msg: Dict = orjson.loads(raw).get("data")

        msg_type: str = msg.get("e")
        if msg_type is None:
            if "lastUpdateId" not in msg:
                tick = parse_quote_tick_ws(
                    msg, symbol=Symbol(msg["s"]), ts_init=self._clock.timestamp_ns()
                )
                self._handle_data(tick)
        elif msg_type == "depthUpdate":
            deltas = parse_diff_depth_stream_ws(
                msg, symbol=Symbol(msg["s"]), ts_init=self._clock.timestamp_ns()
            )
            self._handle_data(deltas)
        elif msg_type == "24hrTicker":
            ticker = parse_ticker_ws(
                msg,
                symbol=Symbol(msg["s"]),
                ts_init=self._clock.timestamp_ns(),
            )
            self._handle_data(ticker)
        elif msg_type == "trade":
            tick = parse_trade_tick_ws(
                msg,
                symbol=Symbol(msg["s"]),
                ts_init=self._clock.timestamp_ns(),
            )
            self._handle_data(tick)
        elif msg_type == "kline":
            kline = msg["k"]
            if msg["E"] < kline["T"]:
                return  # Bar has not closed yet
            bar = parse_bar_ws(msg, kline=kline, ts_init=self._clock.timestamp_ns())
            self._handle_data(bar)
