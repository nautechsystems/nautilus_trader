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
from typing import Any, Dict, List, Optional

import msgspec
import pandas as pd

from nautilus_trader.adapters.ftx.core.constants import FTX_VENUE
from nautilus_trader.adapters.ftx.core.types import FTXTicker
from nautilus_trader.adapters.ftx.http.client import FTXHttpClient
from nautilus_trader.adapters.ftx.http.error import FTXClientError
from nautilus_trader.adapters.ftx.http.error import FTXError
from nautilus_trader.adapters.ftx.parsing.common import parse_instrument
from nautilus_trader.adapters.ftx.parsing.http import parse_bars_http
from nautilus_trader.adapters.ftx.parsing.websocket import parse_book_partial_ws
from nautilus_trader.adapters.ftx.parsing.websocket import parse_book_update_ws
from nautilus_trader.adapters.ftx.parsing.websocket import parse_quote_tick_ws
from nautilus_trader.adapters.ftx.parsing.websocket import parse_ticker_ws
from nautilus_trader.adapters.ftx.parsing.websocket import parse_trade_ticks_ws
from nautilus_trader.adapters.ftx.providers import FTXInstrumentProvider
from nautilus_trader.adapters.ftx.websocket.client import FTXWebSocketClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LogColor
from nautilus_trader.common.logging import Logger
from nautilus_trader.core.datetime import secs_to_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.c_enums.bar_aggregation import BarAggregationParser
from nautilus_trader.model.data.bar import Bar
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.data.base import DataType
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AggregationSource
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.orderbook.data import OrderBookDeltas
from nautilus_trader.model.orderbook.data import OrderBookSnapshot
from nautilus_trader.msgbus.bus import MessageBus


class FTXDataClient(LiveMarketDataClient):
    """
    Provides a data client for the FTX exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : FTXHttpClient
        The FTX HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    logger : Logger
        The logger for the client.
    instrument_provider : FTXInstrumentProvider
        The instrument provider.
    us : bool, default False
        If the client is for FTX US.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: FTXHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
        instrument_provider: FTXInstrumentProvider,
        us: bool = False,
    ):
        super().__init__(
            loop=loop,
            client_id=ClientId(FTX_VENUE.value),
            venue=FTX_VENUE,
            instrument_provider=instrument_provider,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )

        self._http_client = client
        self._ws_client = FTXWebSocketClient(
            loop=loop,
            clock=clock,
            logger=logger,
            msg_handler=self._handle_ws_message,
            reconnect_handler=self._handle_ws_reconnect,
            key=client.api_key,
            secret=client.api_secret,
            us=us,
        )

        # Hot caches
        self._instrument_ids: Dict[str, InstrumentId] = {}

        if us:
            self._log.info("Set FTX US.", LogColor.BLUE)

    def connect(self) -> None:
        self._log.info("Connecting...")
        self._loop.create_task(self._connect())

    def disconnect(self) -> None:
        self._log.info("Disconnecting...")
        self._loop.create_task(self._disconnect())

    async def _connect(self) -> None:
        # Connect HTTP client
        if not self._http_client.connected:
            await self._http_client.connect()
        try:
            await self._instrument_provider.initialize()
        except FTXError as e:
            self._log.exception("Error on connect", e)
            return

        self._send_all_instruments_to_data_engine()

        # Connect WebSocket client
        await self._ws_client.connect(start=True)
        await self._ws_client.subscribe_markets()

        self._set_connected(True)
        self._log.info("Connected.")

    async def _disconnect(self) -> None:
        # Disconnect WebSocket client
        if self._ws_client.is_connected:
            await self._ws_client.disconnect()
            await self._ws_client.close()

        # Disconnect HTTP client
        if self._http_client.connected:
            await self._http_client.disconnect()

        self._set_connected(False)
        self._log.info("Disconnected.")

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    def subscribe_instruments(self) -> None:
        for instrument_id in list(self._instrument_provider.get_all().keys()):
            self._add_subscription_instrument(instrument_id)

    def subscribe_instrument(self, instrument_id: InstrumentId) -> None:
        self._add_subscription_instrument(instrument_id)

    def subscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Optional[int] = None,
        kwargs: dict = None,
    ) -> None:
        if book_type == BookType.L3_MBO:
            self._log.error(
                "Cannot subscribe to orderbook deltas: "
                "L3_MBO data is not published by FTX. "
                "Valid book types are L1_TBBO, L2_MBP.",
            )
            return

        self._loop.create_task(self._ws_client.subscribe_orderbook(instrument_id.symbol.value))
        self._add_subscription_order_book_deltas(instrument_id)

    def subscribe_order_book_snapshots(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Optional[int] = None,
        kwargs: dict = None,
    ) -> None:
        if book_type == BookType.L3_MBO:
            self._log.error(
                "Cannot subscribe to orderbook snapshots: "
                "L3_MBO data is not published by FTX. "
                "Valid book types are L1_TBBO, L2_MBP.",
            )
            return

        self._loop.create_task(self._ws_client.subscribe_orderbook(instrument_id.symbol.value))
        self._add_subscription_order_book_snapshots(instrument_id)

    def subscribe_ticker(self, instrument_id: InstrumentId) -> None:
        self._loop.create_task(self._ws_client.subscribe_ticker(instrument_id.symbol.value))
        self._add_subscription_ticker(instrument_id)

    def subscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        self._loop.create_task(self._ws_client.subscribe_ticker(instrument_id.symbol.value))
        self._add_subscription_quote_ticks(instrument_id)

    def subscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        self._loop.create_task(self._ws_client.subscribe_trades(instrument_id.symbol.value))
        self._add_subscription_trade_ticks(instrument_id)

    def subscribe_bars(self, bar_type: BarType) -> None:
        self._log.error(
            f"Cannot subscribe to bars {bar_type} (not supported by the FTX exchange). "
            "Try and subscribe with `BarType` for INTERNAL aggregation source",
        )

    def subscribe_instrument_status_updates(self, instrument_id: InstrumentId) -> None:
        self._log.error(
            f"Cannot subscribe to instrument status updates for {instrument_id} "
            f"(not yet supported by NautilusTrader).",
        )

    def subscribe_instrument_close_prices(self, instrument_id: InstrumentId) -> None:
        self._log.error(
            f"Cannot subscribe to instrument close prices for {instrument_id} "
            f"(not supported by the FTX exchange).",
        )

    def unsubscribe_instruments(self) -> None:
        for instrument_id in list(self._instrument_provider.get_all().keys()):
            self._remove_subscription_instrument(instrument_id)

    def unsubscribe_instrument(self, instrument_id: InstrumentId) -> None:
        self._remove_subscription_instrument(instrument_id)

    def unsubscribe_order_book_deltas(self, instrument_id: InstrumentId) -> None:
        self._remove_subscription_order_book_deltas(instrument_id)
        if instrument_id not in self.subscribed_order_book_snapshots():
            # Only unsubscribe if there are also no subscriptions for the
            # markets order book snapshots.
            self._loop.create_task(
                self._ws_client.unsubscribe_orderbook(instrument_id.symbol.value)
            )

    def unsubscribe_order_book_snapshots(self, instrument_id: InstrumentId) -> None:
        self._remove_subscription_order_book_snapshots(instrument_id)
        if instrument_id not in self.subscribed_order_book_deltas():
            # Only unsubscribe if there are also no subscriptions for the markets order book deltas
            self._loop.create_task(
                self._ws_client.unsubscribe_orderbook(instrument_id.symbol.value)
            )

    def unsubscribe_ticker(self, instrument_id: InstrumentId) -> None:
        self._remove_subscription_ticker(instrument_id)
        if instrument_id not in self.subscribed_quote_ticks():
            # Only unsubscribe if there are also no subscriptions for the markets quote ticks
            self._loop.create_task(self._ws_client.unsubscribe_ticker(instrument_id.symbol.value))

    def unsubscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        self._remove_subscription_quote_ticks(instrument_id)
        if instrument_id not in self.subscribed_tickers():
            # Only unsubscribe if there are also no subscriptions for the markets ticker
            self._loop.create_task(self._ws_client.unsubscribe_ticker(instrument_id.symbol.value))

    def unsubscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        self._remove_subscription_trade_ticks(instrument_id)
        self._loop.create_task(self._ws_client.unsubscribe_trades(instrument_id.symbol.value))

    def unsubscribe_bars(self, bar_type: BarType) -> None:
        self._log.error(
            f"Cannot unsubscribe from bars {bar_type} (not supported by the FTX exchange)."
        )

    def unsubscribe_instrument_status_updates(self, instrument_id: InstrumentId) -> None:
        self._log.error(
            "Cannot unsubscribe from instrument status updates (not supported by the FTX exchange).",
        )

    def unsubscribe_instrument_close_prices(self, instrument_id: InstrumentId) -> None:
        self._log.error(
            "Cannot unsubscribe from instrument close prices (not supported by the FTX exchange).",
        )

    # -- REQUESTS ---------------------------------------------------------------------------------

    def request_instrument(self, instrument_id: InstrumentId, correlation_id: UUID4) -> None:
        instrument: Optional[Instrument] = self._instrument_provider.find(instrument_id)
        if instrument is None:
            self._log.error(f"Cannot find instrument for {instrument_id}.")
            return

        data_type = DataType(
            type=Instrument,
            metadata={"instrument_id": instrument_id},
        )

        self._handle_data_response(
            data_type=data_type,
            data=[instrument],  # Data engine handles lists of instruments
            correlation_id=correlation_id,
        )

    def request_quote_ticks(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
        from_datetime: Optional[pd.Timestamp] = None,
        to_datetime: Optional[pd.Timestamp] = None,
    ) -> None:
        self._log.error(
            "Cannot request historical quote ticks: not published by FTX.",
        )

    def request_trade_ticks(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
        from_datetime: Optional[pd.Timestamp] = None,
        to_datetime: Optional[pd.Timestamp] = None,
    ) -> None:
        self._loop.create_task(
            self._request_trade_ticks(
                instrument_id,
                limit,
                correlation_id,
                from_datetime,
                to_datetime,
            )
        )

    async def _request_trade_ticks(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
        from_datetime: Optional[pd.Timestamp] = None,
        to_datetime: Optional[pd.Timestamp] = None,
    ) -> None:
        instrument = self._instrument_provider.find(instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot parse trades response: no instrument found for {instrument_id}.",
            )
            return

        data = await self._http_client.get_trades(instrument_id.symbol.value)

        # Limit trades data
        if limit:
            while len(data) > limit:
                data.pop(0)  # Pop left

        ticks: List[TradeTick] = parse_trade_ticks_ws(
            instrument=instrument,
            data=data,
            ts_init=self._clock.timestamp_ns(),
        )

        data_type = DataType(
            type=TradeTick,
            metadata={
                "instrument_id": instrument_id,
                "from_datetime": from_datetime,
                "to_datetime": to_datetime,
            },
        )

        self._handle_data_response(
            data_type=data_type,
            data=ticks,
            correlation_id=correlation_id,
        )

    def request_bars(
        self,
        bar_type: BarType,
        limit: int,
        correlation_id: UUID4,
        from_datetime: Optional[pd.Timestamp] = None,
        to_datetime: Optional[pd.Timestamp] = None,
    ) -> None:
        if not bar_type.spec.is_time_aggregated():
            self._log.error(
                f"Cannot request {bar_type}: only time bars are aggregated by FTX.",
            )
            return

        if bar_type.spec.price_type != PriceType.LAST:
            self._log.error(
                f"Cannot request historical bars for {bar_type}: "
                f"can only request with `price_type` LAST which is based on trades.",
            )
            return

        if bar_type.aggregation_source == AggregationSource.INTERNAL:
            self._log.warning(
                f"Requesting historical bars for {bar_type} "
                f"which has an INTERNAL aggregation source, "
                f"however requested bars were aggregated externally by FTX.",
            )

        self._loop.create_task(
            self._request_bars(
                bar_type,
                limit,
                correlation_id,
                from_datetime,
                to_datetime,
            )
        )

    async def _request_bars(  # noqa C901 'FTXDataClient._request_bars' is too complex (11)
        self,
        bar_type: BarType,
        limit: int,
        correlation_id: UUID4,
        from_datetime: Optional[pd.Timestamp] = None,
        to_datetime: Optional[pd.Timestamp] = None,
    ) -> None:
        instrument = self._instrument_provider.find(bar_type.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot parse historical bars: "
                f"no instrument found for {bar_type.instrument_id}.",
            )
            return

        if bar_type.spec.aggregation == BarAggregation.SECOND:
            resolution = bar_type.spec.step
        elif bar_type.spec.aggregation == BarAggregation.MINUTE:
            resolution = bar_type.spec.step * 60
        elif bar_type.spec.aggregation == BarAggregation.HOUR:
            resolution = bar_type.spec.step * 60 * 60
        elif bar_type.spec.aggregation == BarAggregation.DAY:
            resolution = bar_type.spec.step * 60 * 60 * 24
        else:  # pragma: no cover (design-time error)
            raise RuntimeError(
                f"invalid aggregation type, "
                f"was {BarAggregationParser.to_str_py(bar_type.spec.aggregation)}",
            )

        # Define validation constants
        max_seconds: int = 30 * 86400
        valid_windows: List[int] = [15, 60, 300, 900, 3600, 14400, 86400]

        # Validate resolution
        if resolution > max_seconds:
            self._log.error(
                f"Cannot request bars for {bar_type}: "
                f"seconds window exceeds MAX_SECONDS {max_seconds}.",
            )
            return

        if resolution > 86400 and resolution % 86400 != 0:
            self._log.error(
                f"Cannot request bars for {bar_type}: "
                f"seconds window exceeds 1 day 86,400 and not a multiple of 1 day.",
            )
            return
        elif resolution not in valid_windows:
            self._log.error(
                f"Cannot request bars for {bar_type}: "
                f"invalid seconds window, use one of {valid_windows}.",
            )
            return

        # Get historical bars data
        data: List[Dict[str, Any]] = await self._http_client.get_historical_prices(
            market=bar_type.instrument_id.symbol.value,
            resolution=resolution,
            start_time=int(from_datetime.timestamp()) if from_datetime is not None else None,
            end_time=int(to_datetime.timestamp()) if to_datetime is not None else None,
        )

        # Limit bars data
        if limit:
            while len(data) > limit:
                data.pop(0)  # Pop left

        bars: List[Bar] = parse_bars_http(
            instrument=instrument,
            bar_type=bar_type,
            data=data,
            ts_event_delta=secs_to_nanos(resolution),
            ts_init=self._clock.timestamp_ns(),
        )
        partial: Bar = bars.pop()

        self._handle_bars(bar_type, bars, partial, correlation_id)

    async def _subscribed_instruments_update(self, delay) -> None:
        await self._instrument_provider.load_all_async()

        self._send_all_instruments_to_data_engine()

        update = self.run_after_delay(delay, self._subscribed_instruments_update(delay))
        self._update_instruments_task = self._loop.create_task(update)

    def _send_all_instruments_to_data_engine(self) -> None:
        for instrument in self._instrument_provider.get_all().values():
            self._handle_data(instrument)

        for currency in self._instrument_provider.currencies().values():
            self._cache.add_currency(currency)

    def _get_cached_instrument_id(self, symbol: str) -> InstrumentId:
        # Parse instrument ID
        instrument_id: Optional[InstrumentId] = self._instrument_ids.get(symbol)
        if not instrument_id:
            instrument_id = InstrumentId(Symbol(symbol), FTX_VENUE)
            self._instrument_ids[symbol] = instrument_id
        return instrument_id

    def _handle_ws_reconnect(self) -> None:
        # TODO(cs): Request order book snapshot?
        pass

    def _handle_ws_message(self, raw: bytes) -> None:
        msg: Dict[str, Any] = msgspec.json.decode(raw)
        channel: str = msg.get("channel")
        if channel is None:
            self._log.error(str(msg))
            return

        try:
            if channel == "markets":
                self._loop.create_task(self._handle_markets(msg))
            elif channel == "orderbook":
                self._handle_orderbook(msg)
            elif channel == "ticker":
                self._handle_ticker(msg)
            elif channel == "trades":
                self._handle_trades(msg)
            else:
                self._log.error(f"Unrecognized websocket message type, was {channel}")
        except Exception as e:
            self._log.error(f"Error handling websocket message, {e}")

    async def _handle_markets(self, msg: Dict[str, Any]) -> None:
        data: Optional[Dict[str, Any]] = msg.get("data")
        if data is None:
            self._log.debug(str(data))  # Normally subscription status
            return

        try:
            # Get current commission rates
            account_info: Dict[str, Any] = await self._http_client.get_account_info()
        except FTXClientError as e:
            self._log.error(
                "Cannot load instruments: API key authentication failed "
                f"(this is needed to fetch the applicable account fee tier). {e}",
            )
            return

        data_values = data["data"].values()
        for data in data_values:
            try:
                instrument: Instrument = parse_instrument(
                    account_info=account_info,
                    data=data,
                    ts_init=self._clock.timestamp_ns(),
                )
                self._handle_data(instrument)
            except ValueError as e:
                self._log.warning(
                    f"Unable to parse instrument {data['name']}, {e}.",
                )
                continue

    def _handle_orderbook(self, msg: Dict[str, Any]) -> None:
        data: Optional[Dict[str, Any]] = msg.get("data")
        if data is None:
            self._log.debug(str(data))  # Normally subscription status
            return

        # Get instrument ID
        instrument_id: InstrumentId = self._get_cached_instrument_id(msg["market"])

        msg_type = msg["type"]
        if msg_type == "partial":
            snapshot: OrderBookSnapshot = parse_book_partial_ws(
                instrument_id=instrument_id,
                data=data,
                ts_init=self._clock.timestamp_ns(),
            )
            self._handle_data(snapshot)
        else:  # update
            deltas: OrderBookDeltas = parse_book_update_ws(
                instrument_id=instrument_id, data=data, ts_init=self._clock.timestamp_ns()
            )
            if not deltas.deltas:
                return  # No deltas
            self._handle_data(deltas)

    def _handle_ticker(self, msg: Dict[str, Any]) -> None:
        data: Optional[Dict[str, Any]] = msg.get("data")
        if data is None:
            self._log.debug(str(data))  # Normally subscription status
            return

        # Get instrument
        instrument_id: InstrumentId = self._get_cached_instrument_id(msg["market"])
        instrument: Instrument = self._instrument_provider.find(instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot parse `FTXTicker`: no instrument found for {instrument_id}.",
            )
            return

        tick: QuoteTick = parse_quote_tick_ws(
            instrument=instrument,
            data=data,
            ts_init=self._clock.timestamp_ns(),
        )

        ticker: FTXTicker = parse_ticker_ws(
            instrument=instrument,
            data=data,
            ts_init=self._clock.timestamp_ns(),
        )

        self._handle_data(tick)
        self._handle_data(ticker)

    def _handle_trades(self, msg: Dict[str, Any]) -> None:
        data: Optional[List[Dict[str, Any]]] = msg.get("data")
        if data is None:
            self._log.debug(str(data))  # Normally subscription status
            return

        # Get instrument
        instrument_id: InstrumentId = self._get_cached_instrument_id(msg["market"])
        instrument: Instrument = self._instrument_provider.find(instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot parse `QuoteTick`: no instrument found for {instrument_id}.",
            )
            return

        ticks: List[TradeTick] = parse_trade_ticks_ws(
            instrument=instrument,
            data=data,
            ts_init=self._clock.timestamp_ns(),
        )

        for tick in ticks:
            self._handle_data(tick)
