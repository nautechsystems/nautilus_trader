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
from functools import partial

import msgspec

from nautilus_trader.adapters.okx.common.constants import OKX_VENUE
from nautilus_trader.adapters.okx.common.credentials import get_api_key
from nautilus_trader.adapters.okx.common.credentials import get_api_secret
from nautilus_trader.adapters.okx.common.credentials import get_passphrase
from nautilus_trader.adapters.okx.common.enums import OKXEnumParser
from nautilus_trader.adapters.okx.common.enums import OKXWsBaseUrlType
from nautilus_trader.adapters.okx.common.symbol import OKXSymbol
from nautilus_trader.adapters.okx.config import OKXDataClientConfig
from nautilus_trader.adapters.okx.http.client import OKXHttpClient
from nautilus_trader.adapters.okx.http.market import OKXMarketHttpAPI
from nautilus_trader.adapters.okx.providers import OKXInstrumentProvider
from nautilus_trader.adapters.okx.schemas.ws import OKXWsEventMsg
from nautilus_trader.adapters.okx.schemas.ws import OKXWsGeneralMsg
from nautilus_trader.adapters.okx.schemas.ws import OKXWsOrderbookPushDataMsg
from nautilus_trader.adapters.okx.schemas.ws import OKXWsPushDataMsg
from nautilus_trader.adapters.okx.schemas.ws import OKXWsTradesPushDataMsg
from nautilus_trader.adapters.okx.schemas.ws import decoder_ws_orderbook
from nautilus_trader.adapters.okx.schemas.ws import decoder_ws_trade
from nautilus_trader.adapters.okx.websocket.client import OKX_CHANNEL_WS_BASE_URL_TYPE_MAP
from nautilus_trader.adapters.okx.websocket.client import SUPPORTED_OKX_ORDER_BOOK_DEPTH_CHANNELS
from nautilus_trader.adapters.okx.websocket.client import OKXWebsocketClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.data.messages import RequestBars
from nautilus_trader.data.messages import RequestData
from nautilus_trader.data.messages import RequestInstrument
from nautilus_trader.data.messages import RequestInstruments
from nautilus_trader.data.messages import RequestQuoteTicks
from nautilus_trader.data.messages import RequestTradeTicks
from nautilus_trader.data.messages import SubscribeBars
from nautilus_trader.data.messages import SubscribeOrderBook
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.data.messages import UnsubscribeBars
from nautilus_trader.data.messages import UnsubscribeOrderBook
from nautilus_trader.data.messages import UnsubscribeQuoteTicks
from nautilus_trader.data.messages import UnsubscribeTradeTicks
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument


class OKXDataClient(LiveMarketDataClient):
    """
    Provides a data client for the OKX centralized cypto exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : OKXHttpClient
        The OKX HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : OKXInstrumentProvider
        The instrument provider.
    config : OKXDataClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: OKXHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: OKXInstrumentProvider,
        config: OKXDataClientConfig,
        name: str | None,
    ) -> None:
        self._enum_parser = OKXEnumParser()
        self._instrument_provider: OKXInstrumentProvider  # subclass specific type hinting

        super().__init__(
            loop=loop,
            client_id=ClientId(name or OKX_VENUE.value),
            venue=OKX_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )

        # Cache data config object (because self._config is coerced to dictionary in ``Component``)
        self._data_config = config

        # HTTP API
        self._http_client = client
        self._http_market = OKXMarketHttpAPI(client=client, clock=clock)  # fetch_books

        # WebSocket API
        # Create one client per websocket type
        # self._ws_clients: dict[OKXWsBaseUrlType, list[OKXWebsocketClient]] = defaultdict(list)
        self._ws_clients: dict[OKXWsBaseUrlType, OKXWebsocketClient] = {}
        for ws_base_url_type in set(OKXWsBaseUrlType):
            if ws_base_url_type == OKXWsBaseUrlType.BUSINESS:
                # TODO any future need for business endpoint?
                continue
            ws_client = OKXWebsocketClient(
                clock=clock,
                handler=None,
                handler_reconnect=None,
                api_key=config.api_key or get_api_key(config.is_demo),
                api_secret=config.api_secret or get_api_secret(config.is_demo),
                passphrase=config.passphrase or get_passphrase(config.is_demo),
                base_url=config.base_url_ws,
                ws_base_url_type=ws_base_url_type,
                is_demo=config.is_demo,
                loop=loop,
                login_for_tbt_orderbooks=False,
            )
            ws_client.set_handler(partial(self._handle_ws_message, ws_client))
            self._ws_clients[ws_base_url_type] = ws_client

        # Spin up another websocket client for tick-by-tick books (public but tbt requires login)
        self._ws_client_tbt_books = OKXWebsocketClient(
            clock=clock,
            handler=None,
            handler_reconnect=None,
            api_key=config.api_key or get_api_key(config.is_demo),
            api_secret=config.api_secret or get_api_secret(config.is_demo),
            passphrase=config.passphrase or get_passphrase(config.is_demo),
            base_url=config.base_url_ws,
            ws_base_url_type=OKXWsBaseUrlType.PUBLIC,
            is_demo=config.is_demo,
            loop=loop,
            login_for_tbt_orderbooks=True,
        )
        self._ws_client_tbt_books.set_handler(
            partial(self._handle_ws_message, self._ws_client_tbt_books),
        )

        # Hot cache
        self._last_quotes: dict[InstrumentId, QuoteTick] = {}
        self._bars: dict[BarType, Bar] = {}
        self._book_buffer: dict[InstrumentId, list[OrderBookDeltas]] = {}

        # Hot cache of clients to know how to unsubscribe
        self._tob_client_map: dict[InstrumentId, OKXWebsocketClient] = {}
        self._bar_type_client_map: dict[BarType, OKXWebsocketClient] = {}
        self._depth_client_map: dict[InstrumentId, tuple[int, OKXWebsocketClient]] = {}
        self._trades_client_map: dict[InstrumentId, OKXWebsocketClient] = {}

        # NOTE depth client maintains one subscription of depth > 1 per instrument, the depth value
        # is stored to facilitate unsubscribing

        # WebSocket decoders
        self._decoder_ws_general_msg = msgspec.json.Decoder(OKXWsGeneralMsg)
        self._decoder_ws_event_msg = msgspec.json.Decoder(OKXWsEventMsg)
        self._decoder_ws_push_data_msg = msgspec.json.Decoder(OKXWsPushDataMsg)
        self._decoder_ws_orderbook = decoder_ws_orderbook()
        self._decoder_ws_trade = decoder_ws_trade()

        # Instrument updates
        self._update_instruments_interval_mins: int | None = config.update_instruments_interval_mins
        self._update_instruments_task: asyncio.Task | None = None

    async def _connect(self) -> None:
        await self._instrument_provider.initialize()
        self._send_all_instruments_to_data_engine()

        if self._update_instruments_interval_mins:
            self._update_instruments_task = self.create_task(
                self._update_instruments(self._update_instruments_interval_mins),
            )

        for ws_client in self._ws_clients.values():
            await ws_client.connect()
            await asyncio.sleep(0.5)
        await asyncio.sleep(0.5)
        await self._ws_client_tbt_books.connect()

    async def _disconnect(self) -> None:
        if self._update_instruments_task:
            self._log.debug("Cancelling 'update_instruments' task")
            self._update_instruments_task.cancel()
            self._update_instruments_task = None

        for ws_client in self._ws_clients.values():
            await ws_client.disconnect()
        await self._ws_client_tbt_books.disconnect()

    def _send_all_instruments_to_data_engine(self) -> None:
        for instrument in self._instrument_provider.get_all().values():
            self._handle_data(instrument)

        for currency in self._instrument_provider.currencies().values():
            self._cache.add_currency(currency)

    async def _update_instruments(self, interval_mins: int) -> None:
        try:
            while True:
                self._log.debug(
                    f"Scheduled task 'update_instruments' to run in {interval_mins} minutes",
                )
                await asyncio.sleep(interval_mins * 60)
                await self._instrument_provider.initialize(reload=True)
                self._send_all_instruments_to_data_engine()
        except asyncio.CancelledError:
            self._log.debug("Canceled task 'update_instruments'")

    async def _get_ws_client(
        self,
        ws_base_url_type: OKXWsBaseUrlType,
        tbt_books: bool = False,
    ) -> OKXWebsocketClient:
        ws_client = self._ws_client_tbt_books if tbt_books else self._ws_clients[ws_base_url_type]
        if not ws_client.is_connected:
            await ws_client.connect()
        return ws_client

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    async def _subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        if command.book_type == BookType.L3_MBO:
            self._log.error(
                "Cannot subscribe to order book snapshot/deltas: OKX adapter currently only "
                "supports L1_MBP and L2_MBP book subscriptions.",
            )
            return

        if command.depth == 1 and command.book_type != BookType.L1_MBP:
            self._log.error(
                "Inconsistent argument for `book_type` provided for order book subscription with "
                f"`depth` argument of 1. `book_type` should be {BookType.L1_MBP}, was {command.book_type}",
            )
            return

        if command.depth == 1 or command.book_type == BookType.L1_MBP:
            quote_ticks_subscription = SubscribeQuoteTicks(
                command_id=command.id,
                instrument_id=command.instrument_id,
                client_id=command.client_id,
                venue=command.venue,
                ts_init=command.ts_init,
                params=command.params,
            )
            await self._subscribe_quote_ticks(quote_ticks_subscription)
            return

        if command.depth is None:
            self._log.warning(
                "Depth not prescribed for order book snapshots/deltas subscription for "
                f"{command.instrument_id}. Using default depth of 50.",
            )
            depth = 50

        if command.instrument_id in self._depth_client_map:
            _depth, _ws_client = self._depth_client_map[command.instrument_id]
            if _depth == depth:
                self._log.warning(
                    f"Already subscribed to {command.instrument_id} order books of depth {depth}",
                )
                return
            else:
                self._log.warning(
                    f"Already subscribed to {command.instrument_id} order books of depth {_depth} but "
                    f"requested subscription for depth {depth}. Replacing subscription of "
                    f"depth {_depth} with depth {depth}.",
                )
                order_book_unsubscription = UnsubscribeOrderBook(
                    command_id=command.id,
                    instrument_id=command.instrument_id,
                    client_id=command.client_id,
                    venue=command.venue,
                    ts_init=command.ts_init,
                    params=command.params,
                )
                await self._unsubscribe_order_book_deltas(order_book_unsubscription)

        ws_client = await self._get_ws_client(OKXWsBaseUrlType.PUBLIC, tbt_books=True)

        # Cache the instrument/depth/client combo for unsubscribing
        self._depth_client_map[command.instrument_id] = (depth, ws_client)

        okx_symbol = OKXSymbol(command.instrument_id.symbol.value)
        await ws_client.subscribe_order_book(okx_symbol.raw_symbol, depth)  # type:ignore

        # Start book buffer for instrument in case we receive updates before snapshots
        self._book_buffer[command.instrument_id] = []

    # Copy subscribe method for book deltas to book snapshots (same logic)
    _subscribe_order_book_snapshots = _subscribe_order_book_deltas

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        if command.instrument_id in self._tob_client_map:
            self._log.warning(
                f"Already subscribed to {command.instrument_id} top-of-book (quotes)",
                LogColor.MAGENTA,
            )
            return

        okx_symbol = OKXSymbol(command.instrument_id.symbol.value)
        ws_client = await self._get_ws_client(OKXWsBaseUrlType.PUBLIC, tbt_books=True)
        self._log.debug(
            f"Subscribing quotes {command.instrument_id} via order book deltas",
            LogColor.MAGENTA,
        )
        # Cache the instrument/depth/client combos for unsubscribing
        self._tob_client_map[command.instrument_id] = ws_client
        await ws_client.subscribe_order_book(okx_symbol.raw_symbol, 1)

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        if command.instrument_id in self._trades_client_map:
            self._log.warning(
                f"Already subscribed to {command.instrument_id} trades",
                LogColor.MAGENTA,
            )
            return
        ws_client = await self._get_ws_client(OKXWsBaseUrlType.PUBLIC, tbt_books=False)
        okx_symbol = OKXSymbol(command.instrument_id.symbol.value)
        self._trades_client_map[command.instrument_id] = ws_client
        await ws_client.subscribe_trades(okx_symbol.raw_symbol)

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        PyCondition.is_true(
            command.bar_type.is_externally_aggregated(),
            "aggregation_source is not EXTERNAL",
        )
        self._log.error("OKX bar subscriptions are not yet implemented")

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        self._log.debug(
            f"Unsubscribing {command.instrument_id} from order book deltas/snapshots",
            LogColor.MAGENTA,
        )
        okx_symbol = OKXSymbol(command.instrument_id.symbol.value)

        for iid, (depth, ws_client) in self._depth_client_map.items():
            if iid == command.instrument_id:
                await ws_client.unsubscribe_order_book(okx_symbol.raw_symbol, depth)  # type:ignore
                break
        self._depth_client_map.pop(command.instrument_id, None)

    # Copy unsubscribe method for book deltas to book snapshots (same logic)
    _unsubscribe_order_book_snapshots = _unsubscribe_order_book_deltas

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        self._log.debug(
            f"Unsubscribing {command.instrument_id} from quotes (top-of-book)",
            LogColor.MAGENTA,
        )
        okx_symbol = OKXSymbol(command.instrument_id.symbol.value)

        for iid, ws_client in self._tob_client_map.items():
            if iid == command.instrument_id:
                await ws_client.unsubscribe_order_book(okx_symbol.raw_symbol, 1)
                break
        self._tob_client_map.pop(command.instrument_id, None)

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        self._log.debug(f"Unsubscribing {command.instrument_id} from trades", LogColor.MAGENTA)
        okx_symbol = OKXSymbol(command.instrument_id.symbol.value)

        for iid, ws_client in self._trades_client_map.items():
            if iid == command.instrument_id:
                await ws_client.unsubscribe_trades(okx_symbol.raw_symbol)
                break
        self._tob_client_map.pop(command.instrument_id, None)

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        self._log.error("OKX bar subscriptions are not yet implemented")
        return

    # -- REQUESTS ---------------------------------------------------------------------------------

    async def _request(self, request: RequestData) -> None:
        pass

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

        self._handle_instrument(instrument, request.id, request.params)

    async def _request_instruments(self, request: RequestInstruments) -> None:
        if request.start is not None:
            self._log.warning(
                f"Requesting instruments for {request.venue} with specified `start` which has no effect",
            )

        if request.end is not None:
            self._log.warning(
                f"Requesting instruments for {request.venue} with specified `end` which has no effect",
            )

        all_instruments = self._instrument_provider.get_all()
        target_instruments = []
        for instrument in all_instruments.values():
            if instrument.venue == request.venue:
                target_instruments.append(instrument)

        self._handle_instruments(
            target_instruments,
            request.venue,
            request.id,
            request.params,
        )

    async def _request_quote_ticks(self, request: RequestQuoteTicks) -> None:
        self._log.error(
            "Cannot request historical quotes: not published by OKX. Subscribe to "
            "quotes or L1_MBP order book.",
        )
        return

    async def _request_trade_ticks(self, request: RequestTradeTicks) -> None:
        self._log.error("Cannot request historical trades: not yet implemented for OKX")
        return

    async def _request_bars(self, request: RequestBars) -> None:
        self._log.error("Cannot request historical bars: not yet implemented for OKX")
        return

    # -- WEBSOCKET HANDLERS -----------------------------------------------------------------------

    def _handle_ws_message(self, client: OKXWebsocketClient, raw: bytes) -> None:  # noqa: C901
        if raw == b"pong":
            client._last_pong = self._clock.utc_now()
            return

        try:
            msg = self._decoder_ws_general_msg.decode(raw)
        except Exception as e:
            self._log.error(
                f"Failed to decode websocket general message: {raw.decode()} with error {e}",
            )
            return

        channel: str | None
        if msg.is_event_msg:
            try:
                event_msg = self._decoder_ws_event_msg.decode(raw)
            except Exception as e:
                self._log.error(
                    f"Failed to decode websocket event message: {raw.decode()} with error {e}",
                )
                return

            if event_msg.is_login:
                self._log.info("Login succeeded", LogColor.GREEN)
                return

            if event_msg.is_channel_conn_count_error:
                error_str = event_msg.format_channel_conn_count_error()
                self._log.warning(
                    f"Received websocket channel connection count error: {error_str}. The last "
                    "connection was likely rejected and OKX may in rare cases unsubscribe existing "
                    "connections.",
                )
                return

            if event_msg.is_error:
                error_str = event_msg.format_error()
                self._log.error(f"Received websocket error: {error_str}")
                return

            if event_msg.connCount is not None:
                channel = event_msg.channel  # channel won't be None here
                if channel:
                    ws_base_url_type = OKX_CHANNEL_WS_BASE_URL_TYPE_MAP[channel]
                    assert client.ws_base_url_type == ws_base_url_type, (
                        "The websocket client's base url type does not match the expected base url "
                        f"type for this channel ({channel}), got client type: {ws_base_url_type=} vs. "
                        f"channel inferred type: {ws_base_url_type}."
                    )
                    client.update_channel_count(channel, int(event_msg.connCount))

        elif msg.is_push_data_msg:
            try:
                push_data = self._decoder_ws_push_data_msg.decode(raw)
            except Exception as e:
                self._log.error(
                    f"Failed to decode websocket push data message: {raw.decode()} with error {e}",
                )
                return

            channel = push_data.arg.channel

            DATA_CLIENT_SUPPORTED_PUSH_DATA_CHANNELS = [
                "trades",
                *SUPPORTED_OKX_ORDER_BOOK_DEPTH_CHANNELS.values(),
            ]
            if channel not in DATA_CLIENT_SUPPORTED_PUSH_DATA_CHANNELS:
                self._log.error(
                    f"Received message from non-data channel {channel}. Is this intended for the "
                    f"execution client? Current supported data client push data channels: "
                    f"{DATA_CLIENT_SUPPORTED_PUSH_DATA_CHANNELS}. Raw message: {raw.decode()}",
                )
                return

            # Find instrument
            raw_symbol: str = push_data.arg.instId  # type:ignore
            instrument = self._instrument_provider.find_conditional(raw_symbol)
            if instrument is None:
                self._log.error(
                    f"Could not find instrument for raw symbol {raw_symbol!r}, which is needed to "
                    f"correctly parse push data message: {raw.decode()}",
                )
                return

            if channel == "trades":
                self._handle_trade(instrument, raw)
            elif channel in SUPPORTED_OKX_ORDER_BOOK_DEPTH_CHANNELS.values():
                self._handle_orderbook(instrument, raw)
            # elif channel == "tickers":
            #     tickers_msgs.append(push_data)
            else:
                self._log.error(
                    f"Unknown or unsupported websocket push data message with channel: {channel}",
                )
                return
        else:
            self._log.error(
                f"Cannot handle unknown or unsupported websocket message: {raw.decode()}",
            )

    def _handle_trade(self, instrument: Instrument, raw: bytes) -> None:
        try:
            push_data: OKXWsTradesPushDataMsg = self._decoder_ws_trade.decode(raw)
            for data in push_data.data:
                trade: TradeTick = data.parse_to_trade_tick(
                    instrument_id=instrument.id,
                    price_precision=instrument.price_precision,
                    size_precision=instrument.size_precision,
                    ts_init=self._clock.timestamp_ns(),
                )
                self._handle_data(trade)
        except Exception as e:
            self._log.error(f"Failed to handle trade tick push data: {raw.decode()} with error {e}")

    def _handle_orderbook(self, instrument: Instrument, raw: bytes) -> None:
        book_buffer: list | None
        try:
            push_data: OKXWsOrderbookPushDataMsg = self._decoder_ws_orderbook.decode(raw)
            for book_data in push_data.data:
                if len(book_data.asks) == 0:
                    # OKX sends empty asks/bids to inform user connection is still active (ignore)
                    continue
                if push_data.action == "snapshot":
                    snapshot: OrderBookDeltas = book_data.parse_to_snapshot(
                        instrument_id=instrument.id,
                        price_precision=instrument.price_precision,
                        size_precision=instrument.size_precision,
                        ts_init=self._clock.timestamp_ns(),
                    )
                    self._handle_data(snapshot)

                    # Pop book buffer to send buffered deltas because book is starting over (hence
                    # snapshot)
                    book_buffer = self._book_buffer.pop(instrument.id, [])
                    for deltas in book_buffer:
                        if snapshot and deltas.sequence <= snapshot.sequence:
                            continue
                        self._handle_data(deltas)

                elif push_data.action == "update":
                    deltas = book_data.parse_to_deltas(
                        instrument_id=instrument.id,
                        price_precision=instrument.price_precision,
                        size_precision=instrument.size_precision,
                        ts_init=self._clock.timestamp_ns(),
                    )
                    book_buffer = self._book_buffer.get(instrument.id)
                    if book_buffer is not None:
                        # we've received deltas without a snapshot so buffer the deltas until the
                        # snapshot is received
                        book_buffer.append(deltas)
                        return
                    self._handle_data(deltas)

                # Handle the quote tick
                quote = book_data.parse_to_quote_tick(
                    instrument_id=instrument.id,
                    price_precision=instrument.price_precision,
                    size_precision=instrument.size_precision,
                    last_quote=self._last_quotes.get(instrument.id),
                    ts_init=self._clock.timestamp_ns(),
                )
                if quote:
                    self._last_quotes[instrument.id] = quote
                    self._handle_data(quote)

        except Exception as e:
            self._log.error(f"Failed to handle order book push data: {raw.decode()} with error {e}")
