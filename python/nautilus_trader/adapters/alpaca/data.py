# -------------------------------------------------------------------------------------------------
#  Bot-folio Alpaca Adapter for Nautilus Trader
#  https://github.com/mandeltechnologies/bot-folio
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import asyncio
from datetime import datetime
from decimal import Decimal
from typing import Any

from nautilus_trader.adapters.alpaca.config import AlpacaDataClientConfig
from nautilus_trader.adapters.alpaca.constants import ALPACA_VENUE
from nautilus_trader.adapters.alpaca.http.client import AlpacaHttpClient
from nautilus_trader.adapters.alpaca.providers import AlpacaInstrumentProvider
from nautilus_trader.adapters.alpaca.websocket.data_client import AlpacaDataWebSocketClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.data.messages import SubscribeBars
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.data.messages import UnsubscribeBars
from nautilus_trader.data.messages import UnsubscribeQuoteTicks
from nautilus_trader.data.messages import UnsubscribeTradeTicks
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class AlpacaDataClient(LiveMarketDataClient):
    """
    Provides a data client for Alpaca market data.

    Automatically routes subscriptions to the correct WebSocket endpoint:
    - Crypto symbols (containing "/") -> crypto WebSocket
    - Stock symbols -> stocks WebSocket (iex or sip feed)

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : AlpacaHttpClient
        The Alpaca HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : AlpacaInstrumentProvider
        The instrument provider.
    config : AlpacaDataClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: AlpacaHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: AlpacaInstrumentProvider,
        config: AlpacaDataClientConfig,
        name: str | None = None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or "ALPACA"),
            venue=ALPACA_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
            config=config,
        )

        self._http_client = client
        self._config = config

        # Create WebSocket clients for both stocks and crypto
        # Alpaca uses different endpoints for each asset class
        self._stocks_ws_client = AlpacaDataWebSocketClient(
            api_key=config.api_key,
            api_secret=config.api_secret,
            access_token=config.access_token,
            feed=config.data_feed if config.data_feed != "crypto" else "iex",
            logger=self._log,
        )

        self._crypto_ws_client = AlpacaDataWebSocketClient(
            api_key=config.api_key,
            api_secret=config.api_secret,
            access_token=config.access_token,
            feed="crypto",
            logger=self._log,
        )

        # Set up callbacks for both clients
        self._stocks_ws_client.set_on_quote(self._handle_quote)
        self._stocks_ws_client.set_on_trade(self._handle_trade)
        self._stocks_ws_client.set_on_bar(self._handle_bar)
        self._stocks_ws_client.set_on_error(self._handle_ws_error)

        self._crypto_ws_client.set_on_quote(self._handle_quote)
        self._crypto_ws_client.set_on_trade(self._handle_trade)
        self._crypto_ws_client.set_on_bar(self._handle_bar)
        self._crypto_ws_client.set_on_error(self._handle_ws_error)

        # Track which clients are connected
        self._stocks_ws_connected = False
        self._crypto_ws_connected = False

        # Track subscribed bar types by symbol for proper bar handling
        self._subscribed_bar_types: dict[str, BarType] = {}

    def _is_crypto_symbol(self, symbol: str) -> bool:
        """Check if a symbol is a crypto pair (e.g., BTC/USD)."""
        return "/" in symbol

    def _get_ws_client(self, symbol: str) -> AlpacaDataWebSocketClient:
        """Get the appropriate WebSocket client for a symbol."""
        return self._crypto_ws_client if self._is_crypto_symbol(symbol) else self._stocks_ws_client

    async def _ensure_ws_connected(self, symbol: str) -> None:
        """Ensure the appropriate WebSocket client is connected."""
        if self._is_crypto_symbol(symbol):
            if not self._crypto_ws_connected:
                await self._crypto_ws_client.connect()
                self._crypto_ws_connected = True
                self._log.info("Alpaca crypto WebSocket connected", LogColor.GREEN)
        else:
            if not self._stocks_ws_connected:
                await self._stocks_ws_client.connect()
                self._stocks_ws_connected = True
                self._log.info("Alpaca stocks WebSocket connected", LogColor.GREEN)

    async def _ensure_instrument_loaded(self, instrument_id: InstrumentId) -> None:
        """Ensure an instrument is loaded, loading on-demand if needed."""
        # Check if already in cache
        if self._cache.instrument(instrument_id) is not None:
            return

        # Check if already in provider
        if self._instrument_provider.find(instrument_id) is not None:
            # Add to cache
            instrument = self._instrument_provider.find(instrument_id)
            self._cache.add_instrument(instrument)
            self._handle_data(instrument)
            return

        # Load on-demand from Alpaca
        self._log.info(f"Loading instrument on-demand: {instrument_id}")
        await self._instrument_provider.load_async(instrument_id)

        # Add to cache if successfully loaded
        instrument = self._instrument_provider.find(instrument_id)
        if instrument is not None:
            self._cache.add_instrument(instrument)
            self._handle_data(instrument)
            self._log.info(f"Loaded instrument: {instrument_id}", LogColor.GREEN)
        else:
            self._log.warning(f"Failed to load instrument: {instrument_id}")

    async def _connect(self) -> None:
        """Connect the data client."""
        # Connect HTTP client first (needed for instrument provider)
        await self._http_client.connect()

        # Initialize instrument provider (follows Nautilus convention)
        await self._instrument_provider.initialize()

        # Log configuration
        self._log.info(f"Data feed: {self._config.data_feed}", LogColor.BLUE)
        self._log.info(f"Paper trading: {self._config.paper}", LogColor.BLUE)

        # WebSocket clients are connected lazily when first subscription is made
        self._log.info("Alpaca data client connected", LogColor.GREEN)

    async def _disconnect(self) -> None:
        """Disconnect the data client."""
        if self._stocks_ws_connected:
            await self._stocks_ws_client.disconnect()
            self._stocks_ws_connected = False
        if self._crypto_ws_connected:
            await self._crypto_ws_client.disconnect()
            self._crypto_ws_connected = False
        await self._http_client.disconnect()
        self._log.info("Alpaca data client disconnected")

    # -- Subscriptions ----

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        """Subscribe to quote ticks for an instrument."""
        instrument_id = command.instrument_id
        symbol = instrument_id.symbol.value

        # Ensure instrument is loaded (load on-demand if needed)
        await self._ensure_instrument_loaded(instrument_id)

        await self._ensure_ws_connected(symbol)
        ws_client = self._get_ws_client(symbol)
        await ws_client.subscribe_quotes([symbol])
        self._log.debug(f"Subscribed to quotes for {symbol}")

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        """Subscribe to trade ticks for an instrument."""
        instrument_id = command.instrument_id
        symbol = instrument_id.symbol.value

        # Ensure instrument is loaded (load on-demand if needed)
        await self._ensure_instrument_loaded(instrument_id)

        await self._ensure_ws_connected(symbol)
        ws_client = self._get_ws_client(symbol)
        await ws_client.subscribe_trades([symbol])
        self._log.debug(f"Subscribed to trades for {symbol}")

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        """Subscribe to bars for an instrument."""
        instrument_id = command.bar_type.instrument_id
        symbol = instrument_id.symbol.value

        # Ensure instrument is loaded (load on-demand if needed)
        await self._ensure_instrument_loaded(instrument_id)

        await self._ensure_ws_connected(symbol)
        ws_client = self._get_ws_client(symbol)
        await ws_client.subscribe_bars([symbol])
        # Track the bar type for this symbol so we can create proper Bar objects
        self._subscribed_bar_types[symbol] = command.bar_type
        self._log.info(f"Subscribed {command.bar_type} bars")

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        """Unsubscribe from quote ticks for an instrument."""
        symbol = command.instrument_id.symbol.value
        ws_client = self._get_ws_client(symbol)
        await ws_client.unsubscribe_quotes([symbol])

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        """Unsubscribe from trade ticks for an instrument."""
        symbol = command.instrument_id.symbol.value
        ws_client = self._get_ws_client(symbol)
        await ws_client.unsubscribe_trades([symbol])

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        """Unsubscribe from bars for an instrument."""
        symbol = command.bar_type.instrument_id.symbol.value
        ws_client = self._get_ws_client(symbol)
        await ws_client.unsubscribe_bars([symbol])
        # Remove from tracked bar types
        self._subscribed_bar_types.pop(symbol, None)

    # -- Request handlers ----

    async def _request_bars(
        self,
        bar_type: BarType,
        limit: int,
        correlation_id: UUID4,
        start: datetime | None = None,
        end: datetime | None = None,
    ) -> None:
        """Request historical bars."""
        symbol = bar_type.instrument_id.symbol.value

        # Map bar type to Alpaca timeframe
        timeframe = self._map_bar_type_to_timeframe(bar_type)

        start_str = start.isoformat() if start else None
        end_str = end.isoformat() if end else None

        # Use correct feed for the symbol type
        feed = "crypto" if self._is_crypto_symbol(symbol) else self._config.data_feed

        try:
            response = await self._http_client.get_bars(
                symbol=symbol,
                timeframe=timeframe,
                start=start_str,
                end=end_str,
                limit=limit,
                feed=feed,
            )

            bars = []
            bars_data = response.get("bars", {})
            
            # Handle different response formats:
            # - Stocks: {"bars": [...]} (list)
            # - Crypto: {"bars": {"BTC/USD": [...]}} (dict keyed by symbol)
            if isinstance(bars_data, dict):
                # Crypto format - extract bars for this symbol
                symbol_bars = bars_data.get(symbol, [])
            else:
                # Stocks format - bars is already a list
                symbol_bars = bars_data
                
            for bar_data in symbol_bars:
                bar = self._parse_bar(bar_data, bar_type)
                bars.append(bar)

            self._handle_bars(bar_type, bars, correlation_id)

        except Exception as e:
            self._log.error(f"Failed to request bars: {e}")

    def _map_bar_type_to_timeframe(self, bar_type: BarType) -> str:
        """Map Nautilus BarType to Alpaca timeframe string."""
        # Simple mapping - extend as needed
        step = bar_type.spec.step
        aggregation = str(bar_type.spec.aggregation)

        if aggregation == "MINUTE":
            return f"{step}Min"
        elif aggregation == "HOUR":
            return f"{step}Hour"
        elif aggregation == "DAY":
            return f"{step}Day"
        elif aggregation == "WEEK":
            return f"{step}Week"
        elif aggregation == "MONTH":
            return f"{step}Month"
        else:
            return "1Day"  # Default

    # -- Message handlers ----

    def _handle_quote(self, data: dict[str, Any]) -> None:
        """Handle incoming quote message from WebSocket."""
        try:
            symbol = data.get("S")
            if not symbol:
                return

            instrument_id = InstrumentId(
                symbol=Symbol(symbol),
                venue=ALPACA_VENUE,
            )

            # Parse timestamp
            ts_str = data.get("t")
            ts_event = self._parse_timestamp(ts_str)

            quote = QuoteTick(
                instrument_id=instrument_id,
                bid_price=Price.from_str(str(data.get("bp", 0))),
                ask_price=Price.from_str(str(data.get("ap", 0))),
                bid_size=Quantity.from_str(str(data.get("bs", 0))),
                ask_size=Quantity.from_str(str(data.get("as", 0))),
                ts_event=ts_event,
                ts_init=self._clock.timestamp_ns(),
            )

            self._handle_data(quote)

        except Exception as e:
            self._log.error(f"Error handling quote: {e}")

    def _handle_trade(self, data: dict[str, Any]) -> None:
        """Handle incoming trade message from WebSocket."""
        try:
            symbol = data.get("S")
            if not symbol:
                return

            instrument_id = InstrumentId(
                symbol=Symbol(symbol),
                venue=ALPACA_VENUE,
            )

            # Parse timestamp
            ts_str = data.get("t")
            ts_event = self._parse_timestamp(ts_str)

            trade = TradeTick(
                instrument_id=instrument_id,
                price=Price.from_str(str(data.get("p", 0))),
                size=Quantity.from_str(str(data.get("s", 0))),
                aggressor_side=AggressorSide.NO_AGGRESSOR,
                trade_id=TradeId(str(data.get("i", ""))),
                ts_event=ts_event,
                ts_init=self._clock.timestamp_ns(),
            )

            self._handle_data(trade)

        except Exception as e:
            self._log.error(f"Error handling trade: {e}")

    def _handle_bar(self, data: dict[str, Any]) -> None:
        """Handle incoming bar message from WebSocket."""
        try:
            symbol = data.get("S")
            if not symbol:
                return

            # Look up the subscribed bar type for this symbol
            bar_type = self._subscribed_bar_types.get(symbol)
            if bar_type is None:
                self._log.warning(f"Received bar for unsubscribed symbol: {symbol}")
                return

            # Parse the bar using the known bar type
            bar = self._parse_bar(data, bar_type)
            self._handle_data(bar)

        except Exception as e:
            self._log.error(f"Error handling bar: {e}")

    def _handle_ws_error(self, error: str) -> None:
        """Handle WebSocket error."""
        self._log.error(f"Alpaca data WebSocket error: {error}")

    def _parse_bar(self, data: dict[str, Any], bar_type: BarType) -> Bar:
        """Parse bar data from Alpaca response."""
        ts_str = data.get("t")
        ts_event = self._parse_timestamp(ts_str)

        return Bar(
            bar_type=bar_type,
            open=Price.from_str(str(data.get("o", 0))),
            high=Price.from_str(str(data.get("h", 0))),
            low=Price.from_str(str(data.get("l", 0))),
            close=Price.from_str(str(data.get("c", 0))),
            volume=Quantity.from_str(str(data.get("v", 0))),
            ts_event=ts_event,
            ts_init=self._clock.timestamp_ns(),
        )

    def _parse_timestamp(self, ts_str: str | None) -> int:
        """Parse ISO timestamp string to nanoseconds."""
        if not ts_str:
            return self._clock.timestamp_ns()

        try:
            # Alpaca uses RFC3339 format
            dt = datetime.fromisoformat(ts_str.replace("Z", "+00:00"))
            return dt_to_unix_nanos(dt)
        except Exception:
            return self._clock.timestamp_ns()

