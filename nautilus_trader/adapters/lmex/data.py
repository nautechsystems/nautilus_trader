# -------------------------------------------------------------------------------------------------
#  LMEX NautilusTrader Adapter
#  Licensed under the GNU Lesser General Public License Version 3.0
# -------------------------------------------------------------------------------------------------

"""
LMEX live market data client.

Subscribes to LMEX WebSocket feeds and dispatches trade ticks and order book
updates to the NautilusTrader data engine.
"""

from __future__ import annotations

import asyncio
from typing import Any

import msgspec

from nautilus_trader.adapters.lmex.config import LmexDataClientConfig
from nautilus_trader.adapters.lmex.constants import LMEX_VENUE
from nautilus_trader.adapters.lmex.constants import LMEX_WS_TOPIC_NOTIFICATIONS
from nautilus_trader.adapters.lmex.constants import LMEX_WS_TOPIC_ORDERBOOK
from nautilus_trader.adapters.lmex.constants import LMEX_WS_TOPIC_TRADES
from nautilus_trader.adapters.lmex.http.client import LmexHttpClient
from nautilus_trader.adapters.lmex.http.market import LmexMarketHttpAPI
from nautilus_trader.adapters.lmex.providers import LmexInstrumentProvider
from nautilus_trader.adapters.lmex.schemas.ws import LmexWsMsg
from nautilus_trader.adapters.lmex.schemas.ws import LmexWsOrderBookMsg
from nautilus_trader.adapters.lmex.schemas.ws import LmexWsTradeMsg
from nautilus_trader.adapters.lmex.websocket.client import LmexWebSocketClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.data.messages import RequestOrderBookSnapshot
from nautilus_trader.data.messages import RequestTradeTicks
from nautilus_trader.data.messages import SubscribeOrderBook
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.data.messages import UnsubscribeOrderBook
from nautilus_trader.data.messages import UnsubscribeTradeTicks
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import RecordFlag
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class LmexLiveMarketDataClient(LiveMarketDataClient):
    """
    Provides a live market data client for the LMEX exchange.

    Connects to the LMEX WebSocket API to stream real-time trade ticks and
    order book updates.  Instrument definitions are loaded from the REST API.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    http_client : LmexHttpClient
        The LMEX HTTP client (used for REST requests).
    ws_client : LmexWebSocketClient
        The LMEX WebSocket client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : LmexInstrumentProvider
        The instrument provider.
    config : LmexDataClientConfig
        Configuration for the client.
    name : str or None, optional
        Custom client ID.  Defaults to the venue name ``"LMEX"``.

    """

    # msgspec decoders for WS message dispatch
    _dec_envelope = msgspec.json.Decoder(LmexWsMsg)
    _dec_trade = msgspec.json.Decoder(LmexWsTradeMsg)
    _dec_orderbook = msgspec.json.Decoder(LmexWsOrderBookMsg)

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        http_client: LmexHttpClient,
        ws_client: LmexWebSocketClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: LmexInstrumentProvider,
        config: LmexDataClientConfig,
        name: str | None = None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or LMEX_VENUE.value),
            venue=LMEX_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )

        self._http_client = http_client
        self._ws_client = ws_client
        self._config = config
        self._instrument_provider: LmexInstrumentProvider = instrument_provider
        self._market_api = LmexMarketHttpAPI(http_client)

        self._update_instruments_task: asyncio.Task | None = None

        self._log.info(f"{config.is_sandbox=}", LogColor.BLUE)
        self._log.info(
            f"{config.update_instruments_interval_mins=}",
            LogColor.BLUE,
        )

    # ------------------------------------------------------------------
    # Connect / disconnect
    # ------------------------------------------------------------------

    async def _connect(self) -> None:
        """
        Connect to the LMEX WebSocket and load instrument definitions.

        Loads all instruments from the REST API, publishes them to the data
        engine, then establishes the WebSocket connection.

        """
        self._log.info("Connecting...")

        await self._instrument_provider.initialize()
        self._send_all_instruments_to_data_engine()

        await self._ws_client.connect()

        if self._config.update_instruments_interval_mins:
            self._update_instruments_task = self.create_task(
                self._update_instruments_loop(
                    self._config.update_instruments_interval_mins,
                )
            )

        self._log.info("Connected", LogColor.GREEN)

    async def _disconnect(self) -> None:
        """
        Disconnect from the LMEX WebSocket and cancel background tasks.
        """
        self._log.info("Disconnecting...")

        if self._update_instruments_task:
            self._update_instruments_task.cancel()
            self._update_instruments_task = None

        await self._ws_client.disconnect()
        self._log.info("Disconnected", LogColor.BLUE)

    # ------------------------------------------------------------------
    # Subscribe / unsubscribe — trade ticks
    # ------------------------------------------------------------------

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        """
        Subscribe to the trade feed for the given instrument.

        Parameters
        ----------
        command : SubscribeTradeTicks
            The subscription command.

        """
        symbol = command.instrument_id.symbol.value
        topic = f"{LMEX_WS_TOPIC_TRADES}:{symbol}"
        await self._ws_client.subscribe(topic)

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        """
        Unsubscribe from the trade feed for the given instrument.

        Parameters
        ----------
        command : UnsubscribeTradeTicks
            The unsubscription command.

        """
        symbol = command.instrument_id.symbol.value
        topic = f"{LMEX_WS_TOPIC_TRADES}:{symbol}"
        await self._ws_client.unsubscribe(topic)

    # ------------------------------------------------------------------
    # Subscribe / unsubscribe — order book
    # ------------------------------------------------------------------

    async def _subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        """
        Subscribe to the Level-2 order book feed for the given instrument.

        Parameters
        ----------
        command : SubscribeOrderBook
            The subscription command.

        Notes
        -----
        The LMEX orderbook topic suffix ``_0`` requests the full book.
        The exact topic string is to be confirmed with sandbox credentials.

        """
        symbol = command.instrument_id.symbol.value
        # Suffix _0 = full depth; adjust when confirmed from docs/sandbox
        topic = f"{LMEX_WS_TOPIC_ORDERBOOK}:{symbol}_0"
        await self._ws_client.subscribe(topic)

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        """
        Unsubscribe from the order book feed for the given instrument.

        Parameters
        ----------
        command : UnsubscribeOrderBook
            The unsubscription command.

        """
        symbol = command.instrument_id.symbol.value
        topic = f"{LMEX_WS_TOPIC_ORDERBOOK}:{symbol}_0"
        await self._ws_client.unsubscribe(topic)

    # ------------------------------------------------------------------
    # REST requests
    # ------------------------------------------------------------------

    async def _request_order_book_snapshot(
        self,
        request: RequestOrderBookSnapshot,
    ) -> None:
        """
        Fetch and publish an order book snapshot via REST.

        Parameters
        ----------
        request : RequestOrderBookSnapshot
            The snapshot request.

        """
        symbol = request.instrument_id.symbol.value
        depth = request.limit if request.limit and request.limit > 0 else None

        try:
            raw_book = await self._market_api.get_orderbook(symbol, depth=depth)
        except Exception as exc:
            self._log.error(f"Failed to fetch orderbook for {symbol}: {exc}")
            return

        instrument = self._cache.instrument(request.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot parse orderbook: instrument {request.instrument_id} not in cache"
            )
            return

        ts_now = self._clock.timestamp_ns()
        deltas = self._parse_orderbook_snapshot(
            raw_book.symbol,
            raw_book.buyQuote,
            raw_book.sellQuote,
            instrument_id=request.instrument_id,
            price_precision=instrument.price_precision,
            size_precision=instrument.size_precision,
            ts_event=ts_now,
            ts_init=ts_now,
        )
        self._handle_data(deltas)

    async def _request_trade_ticks(self, request: RequestTradeTicks) -> None:
        """
        Fetch and publish recent trade ticks via REST.

        Parameters
        ----------
        request : RequestTradeTicks
            The trade tick request.

        """
        symbol = request.instrument_id.symbol.value
        count = request.limit if request.limit and request.limit > 0 else None

        try:
            trades = await self._market_api.get_trades(symbol, count=count)
        except Exception as exc:
            self._log.error(f"Failed to fetch trades for {symbol}: {exc}")
            return

        instrument = self._cache.instrument(request.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot parse trades: instrument {request.instrument_id} not in cache"
            )
            return

        for trade in reversed(trades):  # oldest first
            tick = TradeTick(
                instrument_id=request.instrument_id,
                price=Price(trade.price, instrument.price_precision),
                size=Quantity(trade.size, instrument.size_precision),
                aggressor_side=(
                    AggressorSide.BUYER if trade.side == "BUY" else AggressorSide.SELLER
                ),
                trade_id=TradeId(str(trade.serialId)),
                ts_event=millis_to_nanos(trade.timestamp),
                ts_init=self._clock.timestamp_ns(),
            )
            self._handle_data(tick)

    # ------------------------------------------------------------------
    # WebSocket message handler
    # ------------------------------------------------------------------

    def _handle_msg(self, raw: bytes) -> None:
        """
        Route an incoming WebSocket message to the appropriate handler.

        This is the single callback registered with the WebSocket client.
        It first decodes a minimal envelope to determine the message type,
        then delegates to a typed handler.

        Parameters
        ----------
        raw : bytes
            Raw WebSocket frame bytes.

        """
        try:
            envelope = self._dec_envelope.decode(raw)
        except (msgspec.DecodeError, Exception) as exc:
            self._log.warning(f"Failed to decode WS envelope: {exc} | {raw[:200]!r}")
            return

        topic: str | None = envelope.topic
        event: str | None = envelope.event

        if topic is not None:
            if topic.startswith(LMEX_WS_TOPIC_TRADES + ":"):
                self._handle_trade_msg(raw, topic)
            elif topic.startswith(LMEX_WS_TOPIC_ORDERBOOK + ":"):
                self._handle_orderbook_msg(raw, topic)
            elif topic == LMEX_WS_TOPIC_NOTIFICATIONS:
                # Execution events — handled by the execution client
                pass
            else:
                self._log.debug(f"Unhandled WS topic: {topic!r}")
        elif event in ("subscribe", "unsubscribe", "pong"):
            # Ack and heartbeat messages — no action needed
            self._log.debug(f"WS control event: {event!r}")
        else:
            self._log.warning(f"Unhandled WS message: {raw[:200]!r}")

    def _handle_trade_msg(self, raw: bytes, topic: str) -> None:
        """
        Parse and publish trade ticks from a ``tradeHistoryApi`` message.

        Parameters
        ----------
        raw : bytes
            Raw WebSocket frame.
        topic : str
            The message topic (e.g. ``"tradeHistoryApi:BTC-USD"``).

        """
        try:
            msg = self._dec_trade.decode(raw)
        except (msgspec.DecodeError, Exception) as exc:
            self._log.warning(f"Failed to decode trade message: {exc} | {raw[:200]!r}")
            return

        # Resolve symbol from topic (format: "tradeHistoryApi:SYMBOL")
        symbol_str = topic.split(":", 1)[1]
        instrument_id = InstrumentId.from_str(f"{symbol_str}.{LMEX_VENUE.value}")

        instrument = self._cache.instrument(instrument_id)
        if instrument is None:
            self._log.warning(
                f"Received trade for uncached instrument {instrument_id}; skipping"
            )
            return

        ts_init = self._clock.timestamp_ns()

        for datum in msg.data:
            tick = TradeTick(
                instrument_id=instrument_id,
                price=Price(datum.price, instrument.price_precision),
                size=Quantity(datum.size, instrument.size_precision),
                aggressor_side=(
                    AggressorSide.BUYER if datum.side == "BUY" else AggressorSide.SELLER
                ),
                trade_id=TradeId(str(datum.tradeId)),
                ts_event=millis_to_nanos(datum.timestamp),
                ts_init=ts_init,
            )
            self._handle_data(tick)

    def _handle_orderbook_msg(self, raw: bytes, topic: str) -> None:
        """
        Parse and publish order book deltas from an ``orderBookApi`` message.

        Parameters
        ----------
        raw : bytes
            Raw WebSocket frame.
        topic : str
            The message topic (e.g. ``"orderBookApi:BTC-USD_0"``).

        Notes
        -----
        The exact shape of LMEX orderbook WS messages is to be confirmed
        during implementation.  This handler assumes the same structure as
        the REST snapshot (``buyQuote`` / ``sellQuote`` lists).

        """
        try:
            msg = self._dec_orderbook.decode(raw)
        except (msgspec.DecodeError, Exception) as exc:
            self._log.warning(
                f"Failed to decode orderbook message: {exc} | {raw[:200]!r}"
            )
            return

        # Strip depth suffix from topic (e.g. "orderBookApi:BTC-USD_0" → "BTC-USD")
        symbol_str = topic.split(":", 1)[1].rsplit("_", 1)[0]
        instrument_id = InstrumentId.from_str(f"{symbol_str}.{LMEX_VENUE.value}")

        instrument = self._cache.instrument(instrument_id)
        if instrument is None:
            self._log.warning(
                f"Received orderbook for uncached instrument {instrument_id}; skipping"
            )
            return

        ts_event = millis_to_nanos(msg.data.timestamp)
        ts_init = self._clock.timestamp_ns()

        deltas = self._parse_orderbook_snapshot(
            msg.data.symbol,
            msg.data.buyQuote,
            msg.data.sellQuote,
            instrument_id=instrument_id,
            price_precision=instrument.price_precision,
            size_precision=instrument.size_precision,
            ts_event=ts_event,
            ts_init=ts_init,
        )
        self._handle_data(deltas)

    # ------------------------------------------------------------------
    # Parsing helpers
    # ------------------------------------------------------------------

    def _parse_orderbook_snapshot(
        self,
        symbol: str,
        buy_quote: list[Any],
        sell_quote: list[Any],
        instrument_id: InstrumentId,
        price_precision: int,
        size_precision: int,
        ts_event: int,
        ts_init: int,
    ) -> OrderBookDeltas:
        """
        Convert LMEX bid/ask lists into an ``OrderBookDeltas`` snapshot.

        Parameters
        ----------
        symbol : str
            Trading pair (used for logging only).
        buy_quote : list
            Bid price levels (each has ``.price`` and ``.size`` attributes or
            dict keys).
        sell_quote : list
            Ask price levels.
        instrument_id : InstrumentId
            The instrument identifier.
        price_precision : int
            Instrument price decimal precision.
        size_precision : int
            Instrument size decimal precision.
        ts_event : int
            UNIX timestamp (nanoseconds) of the event.
        ts_init : int
            UNIX timestamp (nanoseconds) of initialization.

        Returns
        -------
        OrderBookDeltas

        """
        deltas: list[OrderBookDelta] = []

        def _add_delta(side: OrderSide, price_raw: Any, size_raw: Any) -> None:
            price = Price(float(price_raw), price_precision)
            size = Quantity(float(size_raw), size_precision)
            if size == 0:
                action = BookAction.DELETE
            else:
                action = BookAction.ADD
            order = BookOrder(side=side, price=price, size=size, order_id=0)
            delta = OrderBookDelta(
                instrument_id=instrument_id,
                action=action,
                order=order,
                flags=RecordFlag.F_SNAPSHOT if not deltas else 0,
                sequence=0,
                ts_event=ts_event,
                ts_init=ts_init,
            )
            deltas.append(delta)

        for entry in buy_quote:
            _add_delta(OrderSide.BUY, entry.price, entry.size)
        for entry in sell_quote:
            _add_delta(OrderSide.SELL, entry.price, entry.size)

        return OrderBookDeltas(instrument_id=instrument_id, deltas=deltas)

    # ------------------------------------------------------------------
    # Instrument management
    # ------------------------------------------------------------------

    def _send_all_instruments_to_data_engine(self) -> None:
        """Publish all loaded instruments to the data engine."""
        for currency in self._instrument_provider.currencies().values():
            self._cache.add_currency(currency)

        for instrument in self._instrument_provider.get_all().values():
            self._handle_data(instrument)

    async def _update_instruments_loop(self, interval_mins: int) -> None:
        """
        Periodically reload instruments from the REST API.

        Parameters
        ----------
        interval_mins : int
            Interval between reloads in minutes.

        """
        while True:
            try:
                await asyncio.sleep(interval_mins * 60)
                self._log.info("Refreshing instruments...")
                await self._instrument_provider.initialize(reload=True)
                self._send_all_instruments_to_data_engine()
                self._log.info(
                    f"Instruments refreshed (next in {interval_mins} min)",
                    LogColor.BLUE,
                )
            except asyncio.CancelledError:
                self._log.debug("Instrument update task cancelled")
                return
            except Exception as exc:
                self._log.error(f"Error refreshing instruments: {exc}")
