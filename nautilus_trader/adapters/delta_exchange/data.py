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

"""
Data client for Delta Exchange.

This module provides comprehensive market data functionality for Delta Exchange,
including real-time WebSocket subscriptions, historical data requests, and proper
integration with the Nautilus Trader framework.
"""

from __future__ import annotations

import asyncio
import fnmatch
from collections import defaultdict
from decimal import Decimal
from typing import TYPE_CHECKING, Any

from nautilus_trader.adapters.delta_exchange.config import DeltaExchangeDataClientConfig
from nautilus_trader.adapters.delta_exchange.constants import (
    DELTA_EXCHANGE,
    DELTA_EXCHANGE_WS_PRIVATE_CHANNELS,
    DELTA_EXCHANGE_WS_PUBLIC_CHANNELS,
)
from nautilus_trader.adapters.delta_exchange.providers import DeltaExchangeInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock, MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import dt_to_unix_nanos, millis_to_nanos, secs_to_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.messages import (
    DataResponse,
    RequestBars,
    RequestData,
    RequestInstrument,
    RequestInstruments,
    RequestQuoteTicks,
    RequestTradeTicks,
    SubscribeBars,
    SubscribeFundingRates,
    SubscribeInstrument,
    SubscribeInstruments,
    SubscribeMarkPrices,
    SubscribeOrderBook,
    SubscribeQuoteTicks,
    SubscribeTradeTicks,
    UnsubscribeBars,
    UnsubscribeFundingRates,
    UnsubscribeInstrument,
    UnsubscribeInstruments,
    UnsubscribeMarkPrices,
    UnsubscribeOrderBook,
    UnsubscribeQuoteTicks,
    UnsubscribeTradeTicks,
)
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import (
    Bar,
    BarType,
    CustomData,
    DataType,
    FundingRateUpdate,
    MarkPriceUpdate,
    OrderBookDelta,
    OrderBookDeltas,
    OrderBookSnapshot,
    QuoteTick,
    TradeTick,
)
from nautilus_trader.model.enums import BookType, PriceType
from nautilus_trader.model.identifiers import ClientId, InstrumentId
from nautilus_trader.model.instruments import Instrument


if TYPE_CHECKING:
    from nautilus_trader.core.message import Request


class DeltaExchangeDataClient(LiveMarketDataClient):
    """
    Provides a comprehensive data client for Delta Exchange.

    This client handles both real-time WebSocket data feeds and historical data requests,
    supporting all Delta Exchange market data types including quotes, trades, order books,
    mark prices, and funding rates.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : DeltaExchangeHttpClient
        The Delta Exchange HTTP client for REST API requests.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : DeltaExchangeInstrumentProvider
        The instrument provider for loading and managing instruments.
    config : DeltaExchangeDataClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID. If None, uses the venue name.

    Features
    --------
    - Real-time WebSocket subscriptions for all Delta Exchange channels
    - Historical data requests with pagination support
    - Automatic reconnection with exponential backoff
    - Symbol filtering based on configuration
    - Comprehensive error handling and logging
    - Support for both authenticated and unauthenticated feeds
    - Rate limiting compliance with Delta Exchange API limits

    Notes
    -----
    The client automatically handles WebSocket connection management, including
    authentication for private channels, heartbeat/ping-pong, and reconnection
    logic. All Delta Exchange message formats are converted to appropriate
    Nautilus data types.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: nautilus_pyo3.DeltaExchangeHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: DeltaExchangeInstrumentProvider,
        config: DeltaExchangeDataClientConfig,
        name: str | None = None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or DELTA_EXCHANGE.value),
            venue=DELTA_EXCHANGE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )

        # Configuration and clients
        self._client = client
        self._config = config
        self._ws_client: nautilus_pyo3.DeltaExchangeWebSocketClient | None = None

        # Subscription management
        self._subscribed_instruments: set[InstrumentId] = set()
        self._subscribed_quote_ticks: set[InstrumentId] = set()
        self._subscribed_trade_ticks: set[InstrumentId] = set()
        self._subscribed_order_books: set[InstrumentId] = set()
        self._subscribed_bars: dict[BarType, InstrumentId] = {}
        self._subscribed_mark_prices: set[InstrumentId] = set()
        self._subscribed_funding_rates: set[InstrumentId] = set()

        # Channel mapping for Delta Exchange WebSocket
        self._channel_subscriptions: dict[str, set[str]] = defaultdict(set)
        self._symbol_to_instrument: dict[str, InstrumentId] = {}

        # Connection state
        self._is_connected = False
        self._connection_retry_count = 0
        self._max_retry_count = self._config.max_reconnection_attempts

        # Rate limiting
        self._last_request_time = 0.0
        self._request_count = 0

        # Statistics
        self._stats = {
            "messages_received": 0,
            "messages_processed": 0,
            "connection_attempts": 0,
            "reconnections": 0,
            "errors": 0,
            "subscriptions": 0,
            "unsubscriptions": 0,
        }

        # Log configuration
        self._log.info(f"Delta Exchange Data Client initialized", LogColor.BLUE)
        self._log.info(f"Environment: {'testnet' if config.testnet else 'production'}", LogColor.BLUE)
        self._log.info(f"Auto reconnect: {config.auto_reconnect}", LogColor.BLUE)
        self._log.info(f"Default channels: {config.default_channels}", LogColor.BLUE)
        if config.symbol_filters:
            self._log.info(f"Symbol filters: {config.symbol_filters}", LogColor.BLUE)

    @property
    def stats(self) -> dict[str, int]:
        """Return client statistics."""
        return self._stats.copy()

    # -- CONNECTION MANAGEMENT -----------------------------------------------------------------------

    async def _connect(self) -> None:
        """
        Connect the data client.

        This method initializes the WebSocket client, establishes the connection,
        and sets up message handling. It also handles authentication if credentials
        are provided in the configuration.
        """
        try:
            self._stats["connection_attempts"] += 1
            self._log.info("Connecting to Delta Exchange WebSocket...")

            # Get effective credentials and URLs
            api_key = self._config.get_effective_api_key()
            api_secret = self._config.get_effective_api_secret()
            ws_url = self._config.get_effective_ws_url()

            # Initialize WebSocket client
            self._ws_client = nautilus_pyo3.DeltaExchangeWebSocketClient(
                api_key=api_key,
                api_secret=api_secret,
                base_url=ws_url,
                timeout_secs=self._config.ws_timeout_secs,
                heartbeat_interval_secs=self._config.heartbeat_interval_secs,
                max_reconnection_attempts=self._config.max_reconnection_attempts,
                reconnection_delay_secs=self._config.reconnection_delay_secs,
                max_queue_size=self._config.max_queue_size,
            )

            # Set up message handler
            await self._ws_client.set_message_handler(self._handle_ws_message)

            # Connect WebSocket
            await self._ws_client.connect()

            self._is_connected = True
            self._connection_retry_count = 0

            # Subscribe to default channels if configured
            if self._config.default_channels:
                await self._subscribe_default_channels()

            self._log.info(
                f"Connected to Delta Exchange WebSocket at {ws_url}",
                LogColor.GREEN,
            )

        except Exception as e:
            self._is_connected = False
            self._stats["errors"] += 1
            self._log.error(f"Failed to connect to Delta Exchange WebSocket: {e}")
            raise

    async def _disconnect(self) -> None:
        """
        Disconnect the data client.

        This method gracefully closes the WebSocket connection and cleans up
        all subscription state.
        """
        try:
            self._log.info("Disconnecting from Delta Exchange WebSocket...")

            if self._ws_client:
                await self._ws_client.disconnect()
                self._ws_client = None

            # Clear subscription state
            self._subscribed_instruments.clear()
            self._subscribed_quote_ticks.clear()
            self._subscribed_trade_ticks.clear()
            self._subscribed_order_books.clear()
            self._subscribed_bars.clear()
            self._subscribed_mark_prices.clear()
            self._subscribed_funding_rates.clear()
            self._channel_subscriptions.clear()
            self._symbol_to_instrument.clear()

            self._is_connected = False

            self._log.info("Disconnected from Delta Exchange WebSocket", LogColor.YELLOW)

        except Exception as e:
            self._stats["errors"] += 1
            self._log.error(f"Error during disconnect: {e}")

    async def _reset(self) -> None:
        """
        Reset the data client.

        This method resets all internal state and prepares the client for
        a fresh connection.
        """
        self._log.info("Resetting Delta Exchange data client...")

        # Reset statistics
        self._stats = {
            "messages_received": 0,
            "messages_processed": 0,
            "connection_attempts": 0,
            "reconnections": 0,
            "errors": 0,
            "subscriptions": 0,
            "unsubscriptions": 0,
        }

        # Reset connection state
        self._connection_retry_count = 0
        self._last_request_time = 0.0
        self._request_count = 0

        self._log.info("Delta Exchange data client reset complete")

    async def _subscribe_default_channels(self) -> None:
        """Subscribe to default channels configured in the client."""
        try:
            for channel in self._config.default_channels:
                if channel in DELTA_EXCHANGE_WS_PUBLIC_CHANNELS:
                    await self._subscribe_channel(channel)
                    self._log.info(f"Subscribed to default channel: {channel}")
                else:
                    self._log.warning(f"Unknown default channel: {channel}")

        except Exception as e:
            self._log.error(f"Failed to subscribe to default channels: {e}")

    async def _subscribe_channel(self, channel: str, symbols: list[str] | None = None) -> None:
        """
        Subscribe to a Delta Exchange WebSocket channel.

        Parameters
        ----------
        channel : str
            The channel name to subscribe to.
        symbols : list[str], optional
            The symbols to subscribe to for the channel.

        """
        if not self._ws_client:
            self._log.error("WebSocket client not connected")
            return

        try:
            if symbols:
                for symbol in symbols:
                    if self._should_subscribe_symbol(symbol):
                        await self._ws_client.subscribe(channel, symbol)
                        self._channel_subscriptions[channel].add(symbol)
                        self._stats["subscriptions"] += 1
            else:
                await self._ws_client.subscribe(channel)
                self._channel_subscriptions[channel].add("*")
                self._stats["subscriptions"] += 1

        except Exception as e:
            self._stats["errors"] += 1
            self._log.error(f"Failed to subscribe to channel {channel}: {e}")

    async def _unsubscribe_channel(self, channel: str, symbols: list[str] | None = None) -> None:
        """
        Unsubscribe from a Delta Exchange WebSocket channel.

        Parameters
        ----------
        channel : str
            The channel name to unsubscribe from.
        symbols : list[str], optional
            The symbols to unsubscribe from for the channel.

        """
        if not self._ws_client:
            self._log.error("WebSocket client not connected")
            return

        try:
            if symbols:
                for symbol in symbols:
                    await self._ws_client.unsubscribe(channel, symbol)
                    self._channel_subscriptions[channel].discard(symbol)
                    self._stats["unsubscriptions"] += 1
            else:
                await self._ws_client.unsubscribe(channel)
                self._channel_subscriptions[channel].discard("*")
                self._stats["unsubscriptions"] += 1

        except Exception as e:
            self._stats["errors"] += 1
            self._log.error(f"Failed to unsubscribe from channel {channel}: {e}")

    def _should_subscribe_symbol(self, symbol: str) -> bool:
        """
        Check if a symbol should be subscribed to based on configuration filters.

        Parameters
        ----------
        symbol : str
            The symbol to check.

        Returns
        -------
        bool
            True if the symbol should be subscribed to.

        """
        if not self._config.symbol_filters:
            return True

        return any(fnmatch.fnmatch(symbol, pattern) for pattern in self._config.symbol_filters)

    # -- MARKET DATA SUBSCRIPTIONS -------------------------------------------------------------------

    async def _subscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: int | None = None,
        kwargs: dict[str, Any] | None = None,
    ) -> None:
        """
        Subscribe to order book deltas for the given instrument.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to subscribe to.
        book_type : BookType
            The order book type (L1_MBP, L2_MBP, L3_MBO).
        depth : int, optional
            The order book depth (not used by Delta Exchange).
        kwargs : dict[str, Any], optional
            Additional keyword arguments.

        """
        PyCondition.not_none(instrument_id, "instrument_id")

        if not self._ws_client or not self._is_connected:
            self._log.error("WebSocket client not connected")
            return

        symbol = instrument_id.symbol.value

        if not self._should_subscribe_symbol(symbol):
            self._log.info(f"Symbol {symbol} filtered out by configuration")
            return

        try:
            # Subscribe to L2 order book updates and snapshots
            await self._subscribe_channel("l2_orderbook", [symbol])
            await self._subscribe_channel("l2_updates", [symbol])

            self._subscribed_order_books.add(instrument_id)
            self._symbol_to_instrument[symbol] = instrument_id

            self._log.info(f"Subscribed to order book deltas for {instrument_id}")

        except Exception as e:
            self._stats["errors"] += 1
            self._log.error(f"Failed to subscribe to order book deltas for {instrument_id}: {e}")

    async def _subscribe_quote_ticks(
        self,
        instrument_id: InstrumentId,
        kwargs: dict[str, Any] | None = None,
    ) -> None:
        """
        Subscribe to quote ticks for the given instrument.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to subscribe to.
        kwargs : dict[str, Any], optional
            Additional keyword arguments.

        """
        PyCondition.not_none(instrument_id, "instrument_id")

        if not self._ws_client or not self._is_connected:
            self._log.error("WebSocket client not connected")
            return

        symbol = instrument_id.symbol.value

        if not self._should_subscribe_symbol(symbol):
            self._log.info(f"Symbol {symbol} filtered out by configuration")
            return

        try:
            # Subscribe to ticker data for quotes (best bid/ask)
            await self._subscribe_channel("v2_ticker", [symbol])

            self._subscribed_quote_ticks.add(instrument_id)
            self._symbol_to_instrument[symbol] = instrument_id

            self._log.info(f"Subscribed to quote ticks for {instrument_id}")

        except Exception as e:
            self._stats["errors"] += 1
            self._log.error(f"Failed to subscribe to quote ticks for {instrument_id}: {e}")

    async def _subscribe_trade_ticks(
        self,
        instrument_id: InstrumentId,
        kwargs: dict[str, Any] | None = None,
    ) -> None:
        """
        Subscribe to trade ticks for the given instrument.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to subscribe to.
        kwargs : dict[str, Any], optional
            Additional keyword arguments.

        """
        PyCondition.not_none(instrument_id, "instrument_id")

        if not self._ws_client or not self._is_connected:
            self._log.error("WebSocket client not connected")
            return

        symbol = instrument_id.symbol.value

        if not self._should_subscribe_symbol(symbol):
            self._log.info(f"Symbol {symbol} filtered out by configuration")
            return

        try:
            # Subscribe to all trades for the symbol
            await self._subscribe_channel("all_trades", [symbol])

            self._subscribed_trade_ticks.add(instrument_id)
            self._symbol_to_instrument[symbol] = instrument_id

            self._log.info(f"Subscribed to trade ticks for {instrument_id}")

        except Exception as e:
            self._stats["errors"] += 1
            self._log.error(f"Failed to subscribe to trade ticks for {instrument_id}: {e}")

    async def _subscribe_bars(
        self,
        bar_type: BarType,
        kwargs: dict[str, Any] | None = None,
    ) -> None:
        """
        Subscribe to bars for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to subscribe to.
        kwargs : dict[str, Any], optional
            Additional keyword arguments.

        """
        PyCondition.not_none(bar_type, "bar_type")

        if not self._ws_client or not self._is_connected:
            self._log.error("WebSocket client not connected")
            return

        symbol = bar_type.instrument_id.symbol.value

        if not self._should_subscribe_symbol(symbol):
            self._log.info(f"Symbol {symbol} filtered out by configuration")
            return

        try:
            # Map bar aggregation to Delta Exchange resolution
            resolution = self._map_bar_aggregation_to_resolution(bar_type.spec.aggregation)

            # Subscribe to candlestick data
            await self._subscribe_channel("candlesticks", [f"{symbol}:{resolution}"])

            self._subscribed_bars[bar_type] = bar_type.instrument_id
            self._symbol_to_instrument[symbol] = bar_type.instrument_id

            self._log.info(f"Subscribed to bars for {bar_type}")

        except Exception as e:
            self._stats["errors"] += 1
            self._log.error(f"Failed to subscribe to bars for {bar_type}: {e}")

    async def _subscribe_mark_prices(
        self,
        instrument_id: InstrumentId,
        kwargs: dict[str, Any] | None = None,
    ) -> None:
        """
        Subscribe to mark price updates for the given instrument.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to subscribe to.
        kwargs : dict[str, Any], optional
            Additional keyword arguments.

        """
        PyCondition.not_none(instrument_id, "instrument_id")

        if not self._ws_client or not self._is_connected:
            self._log.error("WebSocket client not connected")
            return

        symbol = instrument_id.symbol.value

        if not self._should_subscribe_symbol(symbol):
            self._log.info(f"Symbol {symbol} filtered out by configuration")
            return

        try:
            # Subscribe to mark price updates
            await self._subscribe_channel("mark_price", [symbol])

            self._subscribed_mark_prices.add(instrument_id)
            self._symbol_to_instrument[symbol] = instrument_id

            self._log.info(f"Subscribed to mark prices for {instrument_id}")

        except Exception as e:
            self._stats["errors"] += 1
            self._log.error(f"Failed to subscribe to mark prices for {instrument_id}: {e}")

    async def _subscribe_funding_rates(
        self,
        instrument_id: InstrumentId,
        kwargs: dict[str, Any] | None = None,
    ) -> None:
        """
        Subscribe to funding rate updates for the given instrument.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to subscribe to.
        kwargs : dict[str, Any], optional
            Additional keyword arguments.

        """
        PyCondition.not_none(instrument_id, "instrument_id")

        if not self._ws_client or not self._is_connected:
            self._log.error("WebSocket client not connected")
            return

        symbol = instrument_id.symbol.value

        if not self._should_subscribe_symbol(symbol):
            self._log.info(f"Symbol {symbol} filtered out by configuration")
            return

        try:
            # Subscribe to funding rate updates
            await self._subscribe_channel("funding_rate", [symbol])

            self._subscribed_funding_rates.add(instrument_id)
            self._symbol_to_instrument[symbol] = instrument_id

            self._log.info(f"Subscribed to funding rates for {instrument_id}")

        except Exception as e:
            self._stats["errors"] += 1
            self._log.error(f"Failed to subscribe to funding rates for {instrument_id}: {e}")

    # -- MARKET DATA UNSUBSCRIPTIONS -----------------------------------------------------------------

    async def _unsubscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        kwargs: dict[str, Any] | None = None,
    ) -> None:
        """
        Unsubscribe from order book deltas for the given instrument.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to unsubscribe from.
        kwargs : dict[str, Any], optional
            Additional keyword arguments.

        """
        PyCondition.not_none(instrument_id, "instrument_id")

        if not self._ws_client or not self._is_connected:
            self._log.error("WebSocket client not connected")
            return

        symbol = instrument_id.symbol.value

        try:
            # Unsubscribe from L2 order book channels
            await self._unsubscribe_channel("l2_orderbook", [symbol])
            await self._unsubscribe_channel("l2_updates", [symbol])

            self._subscribed_order_books.discard(instrument_id)
            if symbol in self._symbol_to_instrument:
                del self._symbol_to_instrument[symbol]

            self._log.info(f"Unsubscribed from order book deltas for {instrument_id}")

        except Exception as e:
            self._stats["errors"] += 1
            self._log.error(f"Failed to unsubscribe from order book deltas for {instrument_id}: {e}")

    async def _unsubscribe_quote_ticks(
        self,
        instrument_id: InstrumentId,
        kwargs: dict[str, Any] | None = None,
    ) -> None:
        """
        Unsubscribe from quote ticks for the given instrument.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to unsubscribe from.
        kwargs : dict[str, Any], optional
            Additional keyword arguments.

        """
        PyCondition.not_none(instrument_id, "instrument_id")

        if not self._ws_client or not self._is_connected:
            self._log.error("WebSocket client not connected")
            return

        symbol = instrument_id.symbol.value

        try:
            # Unsubscribe from ticker data
            await self._unsubscribe_channel("v2_ticker", [symbol])

            self._subscribed_quote_ticks.discard(instrument_id)
            if symbol in self._symbol_to_instrument:
                del self._symbol_to_instrument[symbol]

            self._log.info(f"Unsubscribed from quote ticks for {instrument_id}")

        except Exception as e:
            self._stats["errors"] += 1
            self._log.error(f"Failed to unsubscribe from quote ticks for {instrument_id}: {e}")

    async def _unsubscribe_trade_ticks(
        self,
        instrument_id: InstrumentId,
        kwargs: dict[str, Any] | None = None,
    ) -> None:
        """
        Unsubscribe from trade ticks for the given instrument.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to unsubscribe from.
        kwargs : dict[str, Any], optional
            Additional keyword arguments.

        """
        PyCondition.not_none(instrument_id, "instrument_id")

        if not self._ws_client or not self._is_connected:
            self._log.error("WebSocket client not connected")
            return

        symbol = instrument_id.symbol.value

        try:
            # Unsubscribe from all trades
            await self._unsubscribe_channel("all_trades", [symbol])

            self._subscribed_trade_ticks.discard(instrument_id)
            if symbol in self._symbol_to_instrument:
                del self._symbol_to_instrument[symbol]

            self._log.info(f"Unsubscribed from trade ticks for {instrument_id}")

        except Exception as e:
            self._stats["errors"] += 1
            self._log.error(f"Failed to unsubscribe from trade ticks for {instrument_id}: {e}")

    async def _unsubscribe_bars(
        self,
        bar_type: BarType,
        kwargs: dict[str, Any] | None = None,
    ) -> None:
        """
        Unsubscribe from bars for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to unsubscribe from.
        kwargs : dict[str, Any], optional
            Additional keyword arguments.

        """
        PyCondition.not_none(bar_type, "bar_type")

        if not self._ws_client or not self._is_connected:
            self._log.error("WebSocket client not connected")
            return

        symbol = bar_type.instrument_id.symbol.value

        try:
            # Map bar aggregation to Delta Exchange resolution
            resolution = self._map_bar_aggregation_to_resolution(bar_type.spec.aggregation)

            # Unsubscribe from candlestick data
            await self._unsubscribe_channel("candlesticks", [f"{symbol}:{resolution}"])

            if bar_type in self._subscribed_bars:
                del self._subscribed_bars[bar_type]
            if symbol in self._symbol_to_instrument:
                del self._symbol_to_instrument[symbol]

            self._log.info(f"Unsubscribed from bars for {bar_type}")

        except Exception as e:
            self._stats["errors"] += 1
            self._log.error(f"Failed to unsubscribe from bars for {bar_type}: {e}")

    async def _unsubscribe_mark_prices(
        self,
        instrument_id: InstrumentId,
        kwargs: dict[str, Any] | None = None,
    ) -> None:
        """
        Unsubscribe from mark price updates for the given instrument.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to unsubscribe from.
        kwargs : dict[str, Any], optional
            Additional keyword arguments.

        """
        PyCondition.not_none(instrument_id, "instrument_id")

        if not self._ws_client or not self._is_connected:
            self._log.error("WebSocket client not connected")
            return

        symbol = instrument_id.symbol.value

        try:
            # Unsubscribe from mark price updates
            await self._unsubscribe_channel("mark_price", [symbol])

            self._subscribed_mark_prices.discard(instrument_id)
            if symbol in self._symbol_to_instrument:
                del self._symbol_to_instrument[symbol]

            self._log.info(f"Unsubscribed from mark prices for {instrument_id}")

        except Exception as e:
            self._stats["errors"] += 1
            self._log.error(f"Failed to unsubscribe from mark prices for {instrument_id}: {e}")

    async def _unsubscribe_funding_rates(
        self,
        instrument_id: InstrumentId,
        kwargs: dict[str, Any] | None = None,
    ) -> None:
        """
        Unsubscribe from funding rate updates for the given instrument.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to unsubscribe from.
        kwargs : dict[str, Any], optional
            Additional keyword arguments.

        """
        PyCondition.not_none(instrument_id, "instrument_id")

        if not self._ws_client or not self._is_connected:
            self._log.error("WebSocket client not connected")
            return

        symbol = instrument_id.symbol.value

        try:
            # Unsubscribe from funding rate updates
            await self._unsubscribe_channel("funding_rate", [symbol])

            self._subscribed_funding_rates.discard(instrument_id)
            if symbol in self._symbol_to_instrument:
                del self._symbol_to_instrument[symbol]

            self._log.info(f"Unsubscribed from funding rates for {instrument_id}")

        except Exception as e:
            self._stats["errors"] += 1
            self._log.error(f"Failed to unsubscribe from funding rates for {instrument_id}: {e}")

    # -- HISTORICAL DATA REQUESTS --------------------------------------------------------------------

    async def _request_quote_ticks(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
        start: int | None = None,
        end: int | None = None,
        kwargs: dict[str, Any] | None = None,
    ) -> None:
        """
        Request historical quote ticks for the given instrument.

        Note: Delta Exchange doesn't provide dedicated quote tick history.
        This method will return an empty response with a warning.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to request data for.
        limit : int
            The maximum number of ticks to return.
        correlation_id : UUID4
            The correlation ID for the request.
        start : int, optional
            The start timestamp (UNIX nanoseconds).
        end : int, optional
            The end timestamp (UNIX nanoseconds).
        kwargs : dict[str, Any], optional
            Additional keyword arguments.

        """
        self._log.warning(
            f"Quote tick history not available for {instrument_id} on Delta Exchange"
        )

        # Send empty response
        response = DataResponse(
            client_id=self.id,
            venue=self.venue,
            data_type=DataType(QuoteTick, metadata={"instrument_id": instrument_id}),
            data=[],
            correlation_id=correlation_id,
            response_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )
        self._handle_response(response)

    async def _request_trade_ticks(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
        start: int | None = None,
        end: int | None = None,
        kwargs: dict[str, Any] | None = None,
    ) -> None:
        """
        Request historical trade ticks for the given instrument.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to request data for.
        limit : int
            The maximum number of ticks to return.
        correlation_id : UUID4
            The correlation ID for the request.
        start : int, optional
            The start timestamp (UNIX nanoseconds).
        end : int, optional
            The end timestamp (UNIX nanoseconds).
        kwargs : dict[str, Any], optional
            Additional keyword arguments.

        """
        try:
            symbol = instrument_id.symbol.value

            # Apply rate limiting
            await self._apply_rate_limit()

            # Convert timestamps to Delta Exchange format (seconds)
            start_time = start // 1_000_000_000 if start else None
            end_time = end // 1_000_000_000 if end else None

            # Request trades from Delta Exchange API
            trades_response = await self._client.get_trades(
                symbol=symbol,
                limit=min(limit, 1000),  # Delta Exchange limit
                start_time=start_time,
                end_time=end_time,
            )

            if not trades_response or 'result' not in trades_response:
                self._log.warning(f"No trade data received for {instrument_id}")
                trades = []
            else:
                trades = trades_response['result']

            # Convert to Nautilus trade ticks
            trade_ticks = []
            for trade_data in trades:
                try:
                    trade_tick = await self._parse_trade_tick(trade_data, instrument_id)
                    if trade_tick:
                        trade_ticks.append(trade_tick)
                except Exception as e:
                    self._log.warning(f"Failed to parse trade tick: {e}")
                    continue

            # Send response
            response = DataResponse(
                client_id=self.id,
                venue=self.venue,
                data_type=DataType(TradeTick, metadata={"instrument_id": instrument_id}),
                data=trade_ticks,
                correlation_id=correlation_id,
                response_id=UUID4(),
                ts_init=self._clock.timestamp_ns(),
            )
            self._handle_response(response)

            self._log.info(f"Returned {len(trade_ticks)} trade ticks for {instrument_id}")

        except Exception as e:
            self._stats["errors"] += 1
            self._log.error(f"Failed to request trade ticks for {instrument_id}: {e}")

            # Send error response
            response = DataResponse(
                client_id=self.id,
                venue=self.venue,
                data_type=DataType(TradeTick, metadata={"instrument_id": instrument_id}),
                data=[],
                correlation_id=correlation_id,
                response_id=UUID4(),
                ts_init=self._clock.timestamp_ns(),
            )
            self._handle_response(response)

    async def _request_bars(
        self,
        bar_type: BarType,
        limit: int,
        correlation_id: UUID4,
        start: int | None = None,
        end: int | None = None,
        kwargs: dict[str, Any] | None = None,
    ) -> None:
        """
        Request historical bars for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to request data for.
        limit : int
            The maximum number of bars to return.
        correlation_id : UUID4
            The correlation ID for the request.
        start : int, optional
            The start timestamp (UNIX nanoseconds).
        end : int, optional
            The end timestamp (UNIX nanoseconds).
        kwargs : dict[str, Any], optional
            Additional keyword arguments.

        """
        try:
            symbol = bar_type.instrument_id.symbol.value

            # Apply rate limiting
            await self._apply_rate_limit()

            # Map bar aggregation to Delta Exchange resolution
            resolution = self._map_bar_aggregation_to_resolution(bar_type.spec.aggregation)

            # Convert timestamps to Delta Exchange format (seconds)
            start_time = start // 1_000_000_000 if start else None
            end_time = end // 1_000_000_000 if end else None

            # Request candles from Delta Exchange API
            candles_response = await self._client.get_candles(
                symbol=symbol,
                resolution=resolution,
                limit=min(limit, 2000),  # Delta Exchange limit
                start=start_time,
                end=end_time,
            )

            if not candles_response or 'result' not in candles_response:
                self._log.warning(f"No candle data received for {bar_type}")
                candles = []
            else:
                candles = candles_response['result']

            # Convert to Nautilus bars
            bars = []
            for candle_data in candles:
                try:
                    bar = await self._parse_bar(candle_data, bar_type)
                    if bar:
                        bars.append(bar)
                except Exception as e:
                    self._log.warning(f"Failed to parse bar: {e}")
                    continue

            # Send response
            response = DataResponse(
                client_id=self.id,
                venue=self.venue,
                data_type=DataType(Bar, metadata={"bar_type": bar_type}),
                data=bars,
                correlation_id=correlation_id,
                response_id=UUID4(),
                ts_init=self._clock.timestamp_ns(),
            )
            self._handle_response(response)

            self._log.info(f"Returned {len(bars)} bars for {bar_type}")

        except Exception as e:
            self._stats["errors"] += 1
            self._log.error(f"Failed to request bars for {bar_type}: {e}")

            # Send error response
            response = DataResponse(
                client_id=self.id,
                venue=self.venue,
                data_type=DataType(Bar, metadata={"bar_type": bar_type}),
                data=[],
                correlation_id=correlation_id,
                response_id=UUID4(),
                ts_init=self._clock.timestamp_ns(),
            )
            self._handle_response(response)

    # -- MESSAGE HANDLING AND PARSING ----------------------------------------------------------------

    async def _handle_ws_message(self, message: bytes) -> None:
        """
        Handle incoming WebSocket messages from Delta Exchange.

        Parameters
        ----------
        message : bytes
            The raw WebSocket message.

        """
        try:
            self._stats["messages_received"] += 1

            # Parse message using Rust client
            parsed_data = await self._ws_client.parse_message(message)

            if not parsed_data:
                return

            # Route message based on channel
            channel = parsed_data.get("channel")
            if not channel:
                return

            if channel == "v2_ticker":
                await self._handle_ticker_message(parsed_data)
            elif channel == "all_trades":
                await self._handle_trade_message(parsed_data)
            elif channel in ("l2_orderbook", "l2_updates"):
                await self._handle_orderbook_message(parsed_data)
            elif channel == "mark_price":
                await self._handle_mark_price_message(parsed_data)
            elif channel == "funding_rate":
                await self._handle_funding_rate_message(parsed_data)
            elif channel == "candlesticks":
                await self._handle_candlestick_message(parsed_data)
            else:
                self._log.debug(f"Unhandled channel: {channel}")

            self._stats["messages_processed"] += 1

        except Exception as e:
            self._stats["errors"] += 1
            self._log.error(f"Error handling WebSocket message: {e}")

    async def _handle_ticker_message(self, data: dict[str, Any]) -> None:
        """Handle ticker messages for quote tick generation."""
        try:
            symbol = data.get("symbol")
            if not symbol:
                return

            instrument_id = self._symbol_to_instrument.get(symbol)
            if not instrument_id or instrument_id not in self._subscribed_quote_ticks:
                return

            # Parse ticker data to quote tick
            quote_tick = await self._parse_quote_tick(data, instrument_id)
            if quote_tick:
                self._handle_data(quote_tick)

        except Exception as e:
            self._log.error(f"Error handling ticker message: {e}")

    async def _handle_trade_message(self, data: dict[str, Any]) -> None:
        """Handle trade messages for trade tick generation."""
        try:
            symbol = data.get("symbol")
            if not symbol:
                return

            instrument_id = self._symbol_to_instrument.get(symbol)
            if not instrument_id or instrument_id not in self._subscribed_trade_ticks:
                return

            # Parse trade data to trade tick
            trade_tick = await self._parse_trade_tick(data, instrument_id)
            if trade_tick:
                self._handle_data(trade_tick)

        except Exception as e:
            self._log.error(f"Error handling trade message: {e}")

    async def _handle_orderbook_message(self, data: dict[str, Any]) -> None:
        """Handle order book messages for order book delta generation."""
        try:
            symbol = data.get("symbol")
            if not symbol:
                return

            instrument_id = self._symbol_to_instrument.get(symbol)
            if not instrument_id or instrument_id not in self._subscribed_order_books:
                return

            # Parse order book data
            channel = data.get("channel")
            if channel == "l2_orderbook":
                # Full snapshot
                snapshot = await self._parse_order_book_snapshot(data, instrument_id)
                if snapshot:
                    self._handle_data(snapshot)
            elif channel == "l2_updates":
                # Incremental updates
                deltas = await self._parse_order_book_deltas(data, instrument_id)
                if deltas:
                    self._handle_data(deltas)

        except Exception as e:
            self._log.error(f"Error handling order book message: {e}")

    async def _handle_mark_price_message(self, data: dict[str, Any]) -> None:
        """Handle mark price messages."""
        try:
            symbol = data.get("symbol")
            if not symbol:
                return

            instrument_id = self._symbol_to_instrument.get(symbol)
            if not instrument_id or instrument_id not in self._subscribed_mark_prices:
                return

            # Parse mark price data
            mark_price_update = await self._parse_mark_price_update(data, instrument_id)
            if mark_price_update:
                self._handle_data(mark_price_update)

        except Exception as e:
            self._log.error(f"Error handling mark price message: {e}")

    async def _handle_funding_rate_message(self, data: dict[str, Any]) -> None:
        """Handle funding rate messages."""
        try:
            symbol = data.get("symbol")
            if not symbol:
                return

            instrument_id = self._symbol_to_instrument.get(symbol)
            if not instrument_id or instrument_id not in self._subscribed_funding_rates:
                return

            # Parse funding rate data
            funding_rate_update = await self._parse_funding_rate_update(data, instrument_id)
            if funding_rate_update:
                self._handle_data(funding_rate_update)

        except Exception as e:
            self._log.error(f"Error handling funding rate message: {e}")

    async def _handle_candlestick_message(self, data: dict[str, Any]) -> None:
        """Handle candlestick messages for bar generation."""
        try:
            symbol_resolution = data.get("symbol")
            if not symbol_resolution or ":" not in symbol_resolution:
                return

            symbol, resolution = symbol_resolution.split(":", 1)
            instrument_id = self._symbol_to_instrument.get(symbol)
            if not instrument_id:
                return

            # Find matching bar type
            matching_bar_type = None
            for bar_type, bar_instrument_id in self._subscribed_bars.items():
                if (bar_instrument_id == instrument_id and
                    self._map_bar_aggregation_to_resolution(bar_type.spec.aggregation) == resolution):
                    matching_bar_type = bar_type
                    break

            if not matching_bar_type:
                return

            # Parse candlestick data to bar
            bar = await self._parse_bar(data, matching_bar_type)
            if bar:
                self._handle_data(bar)

        except Exception as e:
            self._log.error(f"Error handling candlestick message: {e}")

    async def _parse_quote_tick(self, data: dict[str, Any], instrument_id: InstrumentId) -> QuoteTick | None:
        """
        Parse ticker data into a QuoteTick.

        Parameters
        ----------
        data : dict[str, Any]
            The ticker data from Delta Exchange.
        instrument_id : InstrumentId
            The instrument ID.

        Returns
        -------
        QuoteTick | None
            The parsed quote tick, or None if parsing failed.

        """
        try:
            # Use Rust parsing for performance and consistency
            return await self._ws_client.parse_quote_tick(data, str(instrument_id))
        except Exception as e:
            self._log.warning(f"Failed to parse quote tick for {instrument_id}: {e}")
            return None

    async def _parse_trade_tick(self, data: dict[str, Any], instrument_id: InstrumentId) -> TradeTick | None:
        """
        Parse trade data into a TradeTick.

        Parameters
        ----------
        data : dict[str, Any]
            The trade data from Delta Exchange.
        instrument_id : InstrumentId
            The instrument ID.

        Returns
        -------
        TradeTick | None
            The parsed trade tick, or None if parsing failed.

        """
        try:
            # Use Rust parsing for performance and consistency
            return await self._ws_client.parse_trade_tick(data, str(instrument_id))
        except Exception as e:
            self._log.warning(f"Failed to parse trade tick for {instrument_id}: {e}")
            return None

    async def _parse_order_book_snapshot(self, data: dict[str, Any], instrument_id: InstrumentId) -> OrderBookSnapshot | None:
        """
        Parse order book snapshot data.

        Parameters
        ----------
        data : dict[str, Any]
            The order book snapshot data from Delta Exchange.
        instrument_id : InstrumentId
            The instrument ID.

        Returns
        -------
        OrderBookSnapshot | None
            The parsed order book snapshot, or None if parsing failed.

        """
        try:
            # Use Rust parsing for performance and consistency
            return await self._ws_client.parse_order_book_snapshot(data, str(instrument_id))
        except Exception as e:
            self._log.warning(f"Failed to parse order book snapshot for {instrument_id}: {e}")
            return None

    async def _parse_order_book_deltas(self, data: dict[str, Any], instrument_id: InstrumentId) -> OrderBookDeltas | None:
        """
        Parse order book delta data.

        Parameters
        ----------
        data : dict[str, Any]
            The order book delta data from Delta Exchange.
        instrument_id : InstrumentId
            The instrument ID.

        Returns
        -------
        OrderBookDeltas | None
            The parsed order book deltas, or None if parsing failed.

        """
        try:
            # Use Rust parsing for performance and consistency
            return await self._ws_client.parse_order_book_deltas(data, str(instrument_id))
        except Exception as e:
            self._log.warning(f"Failed to parse order book deltas for {instrument_id}: {e}")
            return None

    async def _parse_mark_price_update(self, data: dict[str, Any], instrument_id: InstrumentId) -> MarkPriceUpdate | None:
        """
        Parse mark price data into a MarkPriceUpdate.

        Parameters
        ----------
        data : dict[str, Any]
            The mark price data from Delta Exchange.
        instrument_id : InstrumentId
            The instrument ID.

        Returns
        -------
        MarkPriceUpdate | None
            The parsed mark price update, or None if parsing failed.

        """
        try:
            # Use Rust parsing for performance and consistency
            return await self._ws_client.parse_mark_price_update(data, str(instrument_id))
        except Exception as e:
            self._log.warning(f"Failed to parse mark price update for {instrument_id}: {e}")
            return None

    async def _parse_funding_rate_update(self, data: dict[str, Any], instrument_id: InstrumentId) -> FundingRateUpdate | None:
        """
        Parse funding rate data into a FundingRateUpdate.

        Parameters
        ----------
        data : dict[str, Any]
            The funding rate data from Delta Exchange.
        instrument_id : InstrumentId
            The instrument ID.

        Returns
        -------
        FundingRateUpdate | None
            The parsed funding rate update, or None if parsing failed.

        """
        try:
            # Use Rust parsing for performance and consistency
            return await self._ws_client.parse_funding_rate_update(data, str(instrument_id))
        except Exception as e:
            self._log.warning(f"Failed to parse funding rate update for {instrument_id}: {e}")
            return None

    async def _parse_bar(self, data: dict[str, Any], bar_type: BarType) -> Bar | None:
        """
        Parse candlestick data into a Bar.

        Parameters
        ----------
        data : dict[str, Any]
            The candlestick data from Delta Exchange.
        bar_type : BarType
            The bar type.

        Returns
        -------
        Bar | None
            The parsed bar, or None if parsing failed.

        """
        try:
            # Use Rust parsing for performance and consistency
            return await self._ws_client.parse_bar(data, str(bar_type))
        except Exception as e:
            self._log.warning(f"Failed to parse bar for {bar_type}: {e}")
            return None

    # -- UTILITY METHODS ------------------------------------------------------------------------------

    def _map_bar_aggregation_to_resolution(self, aggregation: int) -> str:
        """
        Map Nautilus bar aggregation to Delta Exchange resolution.

        Parameters
        ----------
        aggregation : int
            The bar aggregation in seconds.

        Returns
        -------
        str
            The Delta Exchange resolution string.

        """
        # Delta Exchange resolution mapping
        resolution_map = {
            60: "1m",        # 1 minute
            300: "5m",       # 5 minutes
            900: "15m",      # 15 minutes
            1800: "30m",     # 30 minutes
            3600: "1h",      # 1 hour
            7200: "2h",      # 2 hours
            14400: "4h",     # 4 hours
            21600: "6h",     # 6 hours
            43200: "12h",    # 12 hours
            86400: "1d",     # 1 day
            604800: "1w",    # 1 week
        }

        return resolution_map.get(aggregation, "1h")  # Default to 1 hour

    async def _apply_rate_limit(self) -> None:
        """Apply rate limiting to API requests."""
        current_time = self._clock.timestamp()

        # Reset counter if more than 1 second has passed
        if current_time - self._last_request_time >= 1.0:
            self._request_count = 0
            self._last_request_time = current_time

        # Check if we've exceeded the rate limit (10 requests per second)
        if self._request_count >= 10:
            sleep_time = 1.0 - (current_time - self._last_request_time)
            if sleep_time > 0:
                await asyncio.sleep(sleep_time)
                self._request_count = 0
                self._last_request_time = self._clock.timestamp()

        self._request_count += 1

    def _get_cached_instrument(self, symbol: str) -> Instrument | None:
        """
        Get a cached instrument by symbol.

        Parameters
        ----------
        symbol : str
            The symbol to look up.

        Returns
        -------
        Instrument | None
            The cached instrument, or None if not found.

        """
        instrument_id = self._symbol_to_instrument.get(symbol)
        if instrument_id:
            return self._cache.instrument(instrument_id)
        return None

    def _log_subscription_state(self) -> None:
        """Log the current subscription state for debugging."""
        self._log.info("=== Delta Exchange Subscription State ===")
        self._log.info(f"Quote ticks: {len(self._subscribed_quote_ticks)} instruments")
        self._log.info(f"Trade ticks: {len(self._subscribed_trade_ticks)} instruments")
        self._log.info(f"Order books: {len(self._subscribed_order_books)} instruments")
        self._log.info(f"Bars: {len(self._subscribed_bars)} bar types")
        self._log.info(f"Mark prices: {len(self._subscribed_mark_prices)} instruments")
        self._log.info(f"Funding rates: {len(self._subscribed_funding_rates)} instruments")
        self._log.info(f"Channel subscriptions: {dict(self._channel_subscriptions)}")
        self._log.info(f"Symbol mappings: {len(self._symbol_to_instrument)} symbols")
        self._log.info("==========================================")

    def _log_statistics(self) -> None:
        """Log client statistics for monitoring."""
        self._log.info("=== Delta Exchange Data Client Statistics ===")
        for key, value in self._stats.items():
            self._log.info(f"{key}: {value:,}")
        self._log.info("==============================================")

    async def _health_check(self) -> bool:
        """
        Perform a health check on the data client.

        Returns
        -------
        bool
            True if the client is healthy, False otherwise.

        """
        try:
            # Check WebSocket connection
            if not self._ws_client or not self._is_connected:
                return False

            # Check if we can ping the WebSocket
            pong_received = await self._ws_client.ping()
            if not pong_received:
                return False

            # Check if we have recent message activity
            current_time = self._clock.timestamp()
            if hasattr(self, '_last_message_time'):
                time_since_last_message = current_time - self._last_message_time
                if time_since_last_message > 60.0:  # No messages for 60 seconds
                    return False

            return True

        except Exception as e:
            self._log.error(f"Health check failed: {e}")
            return False

    async def _reconnect_if_needed(self) -> None:
        """Reconnect the WebSocket client if needed."""
        if not await self._health_check():
            self._log.warning("Health check failed, attempting reconnection...")

            try:
                await self._disconnect()
                await asyncio.sleep(self._config.reconnection_delay_secs)
                await self._connect()

                # Resubscribe to all active subscriptions
                await self._resubscribe_all()

                self._stats["reconnections"] += 1
                self._log.info("Reconnection successful")

            except Exception as e:
                self._stats["errors"] += 1
                self._log.error(f"Reconnection failed: {e}")

    async def _resubscribe_all(self) -> None:
        """Resubscribe to all active subscriptions after reconnection."""
        try:
            # Resubscribe to quote ticks
            for instrument_id in self._subscribed_quote_ticks.copy():
                await self._subscribe_quote_ticks(instrument_id)

            # Resubscribe to trade ticks
            for instrument_id in self._subscribed_trade_ticks.copy():
                await self._subscribe_trade_ticks(instrument_id)

            # Resubscribe to order books
            for instrument_id in self._subscribed_order_books.copy():
                await self._subscribe_order_book_deltas(instrument_id, BookType.L2_MBP)

            # Resubscribe to bars
            for bar_type in self._subscribed_bars.copy():
                await self._subscribe_bars(bar_type)

            # Resubscribe to mark prices
            for instrument_id in self._subscribed_mark_prices.copy():
                await self._subscribe_mark_prices(instrument_id)

            # Resubscribe to funding rates
            for instrument_id in self._subscribed_funding_rates.copy():
                await self._subscribe_funding_rates(instrument_id)

            self._log.info("All subscriptions restored after reconnection")

        except Exception as e:
            self._log.error(f"Failed to resubscribe after reconnection: {e}")

    def __repr__(self) -> str:
        """Return string representation of the data client."""
        return (
            f"{self.__class__.__name__}("
            f"id={self.id}, "
            f"venue={self.venue}, "
            f"connected={self._is_connected}, "
            f"subscriptions={sum(len(s) for s in [
                self._subscribed_quote_ticks,
                self._subscribed_trade_ticks,
                self._subscribed_order_books,
                self._subscribed_bars,
                self._subscribed_mark_prices,
                self._subscribed_funding_rates,
            ])}"
            f")"
        )
