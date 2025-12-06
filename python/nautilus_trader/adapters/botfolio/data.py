# -------------------------------------------------------------------------------------------------
#  Bot-folio Local Paper Trading Adapter for Nautilus Trader
#  https://github.com/mandeltechnologies/bot-folio
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import asyncio
import json
from datetime import datetime
from typing import Any

import redis.asyncio as aioredis

from nautilus_trader.adapters.botfolio.config import BotfolioDataClientConfig
from nautilus_trader.adapters.botfolio.constants import BOTFOLIO_VENUE
from nautilus_trader.adapters.botfolio.constants import REDIS_BAR_CHANNEL_PREFIX
from nautilus_trader.adapters.botfolio.constants import REDIS_QUOTE_CHANNEL_PREFIX
from nautilus_trader.adapters.botfolio.providers import BotfolioInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.messages import SubscribeBars
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.data.messages import UnsubscribeBars
from nautilus_trader.data.messages import UnsubscribeQuoteTicks
from nautilus_trader.data.messages import UnsubscribeTradeTicks
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class BotfolioDataClient(LiveMarketDataClient):
    """
    Provides a data client for Botfolio local paper trading.

    Subscribes to Redis pub/sub channels for market data published by the
    bot-folio backend (from EODHD).

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : BotfolioInstrumentProvider
        The instrument provider.
    config : BotfolioDataClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: BotfolioInstrumentProvider,
        config: BotfolioDataClientConfig,
        name: str | None = None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or "BOTFOLIO"),
            venue=BOTFOLIO_VENUE,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
            config=config,
        )

        self._config = config
        self._redis_url = config.redis_url

        # Redis pub/sub client
        self._redis: aioredis.Redis | None = None
        self._pubsub: aioredis.client.PubSub | None = None
        self._listen_task: asyncio.Task | None = None

        # Track subscriptions
        self._subscribed_bar_symbols: set[str] = set()
        self._subscribed_quote_symbols: set[str] = set()
        self._bar_types: dict[str, BarType] = {}  # symbol -> BarType mapping

    async def _connect(self) -> None:
        """Connect the data client."""
        self._redis = aioredis.from_url(self._redis_url, decode_responses=True)
        self._pubsub = self._redis.pubsub()

        # Start listening task
        self._listen_task = asyncio.create_task(self._listen_loop())

        self._log.info("Botfolio data client connected", LogColor.GREEN)

    async def _disconnect(self) -> None:
        """Disconnect the data client."""
        if self._listen_task:
            self._listen_task.cancel()
            try:
                await self._listen_task
            except asyncio.CancelledError:
                pass
            self._listen_task = None

        if self._pubsub:
            await self._pubsub.unsubscribe()
            await self._pubsub.close()
            self._pubsub = None

        if self._redis:
            await self._redis.close()
            self._redis = None

        self._subscribed_bar_symbols.clear()
        self._subscribed_quote_symbols.clear()
        self._bar_types.clear()

        self._log.info("Botfolio data client disconnected")

    async def _listen_loop(self) -> None:
        """Listen for Redis pub/sub messages."""
        if not self._pubsub:
            return

        try:
            async for message in self._pubsub.listen():
                if message["type"] == "message":
                    channel = message["channel"]
                    data = message["data"]
                    await self._handle_message(channel, data)
        except asyncio.CancelledError:
            pass
        except Exception as e:
            self._log.error(f"Error in Redis listen loop: {e}")

    async def _handle_message(self, channel: str, data: str) -> None:
        """Handle incoming Redis message."""
        try:
            payload = json.loads(data)

            if channel.startswith(REDIS_BAR_CHANNEL_PREFIX):
                symbol = channel[len(REDIS_BAR_CHANNEL_PREFIX):]
                self._handle_bar_message(symbol, payload)
            elif channel.startswith(REDIS_QUOTE_CHANNEL_PREFIX):
                symbol = channel[len(REDIS_QUOTE_CHANNEL_PREFIX):]
                self._handle_quote_message(symbol, payload)

        except Exception as e:
            self._log.error(f"Error handling message from {channel}: {e}")

    def _handle_bar_message(self, symbol: str, data: dict[str, Any]) -> None:
        """Handle incoming bar message from Redis."""
        bar_type = self._bar_types.get(symbol)
        if not bar_type:
            # Create default 1-minute bar type for this symbol
            instrument_id = InstrumentId(
                symbol=Symbol(symbol),
                venue=BOTFOLIO_VENUE,
            )
            bar_type = BarType.from_str(f"{instrument_id}-1-MINUTE-LAST-EXTERNAL")
            self._bar_types[symbol] = bar_type

        # Parse timestamp
        ts_str = data.get("timestamp")
        ts_event = self._parse_timestamp(ts_str)

        bar = Bar(
            bar_type=bar_type,
            open=Price.from_str(str(data.get("open", 0))),
            high=Price.from_str(str(data.get("high", 0))),
            low=Price.from_str(str(data.get("low", 0))),
            close=Price.from_str(str(data.get("close", 0))),
            volume=Quantity.from_str(str(data.get("volume", 0))),
            ts_event=ts_event,
            ts_init=self._clock.timestamp_ns(),
        )

        self._handle_data(bar)

    def _handle_quote_message(self, symbol: str, data: dict[str, Any]) -> None:
        """Handle incoming quote message from Redis."""
        instrument_id = InstrumentId(
            symbol=Symbol(symbol),
            venue=BOTFOLIO_VENUE,
        )

        price = data.get("price", 0)
        ts_event = self._parse_timestamp_ms(data.get("timestamp", 0))

        # Create a quote tick with bid/ask spread around the price
        # For simplicity, use the same price for bid/ask (no spread)
        quote = QuoteTick(
            instrument_id=instrument_id,
            bid_price=Price.from_str(str(price)),
            ask_price=Price.from_str(str(price)),
            bid_size=Quantity.from_str(str(data.get("volume", 1))),
            ask_size=Quantity.from_str(str(data.get("volume", 1))),
            ts_event=ts_event,
            ts_init=self._clock.timestamp_ns(),
        )

        self._handle_data(quote)

        # Also create a trade tick from the quote
        trade = TradeTick(
            instrument_id=instrument_id,
            price=Price.from_str(str(price)),
            size=Quantity.from_str(str(data.get("volume", 1))),
            aggressor_side=AggressorSide.NO_AGGRESSOR,
            trade_id=TradeId(str(UUID4())),
            ts_event=ts_event,
            ts_init=self._clock.timestamp_ns(),
        )

        self._handle_data(trade)

    def _parse_timestamp(self, ts_str: str | None) -> int:
        """Parse ISO timestamp string to nanoseconds."""
        if not ts_str:
            return self._clock.timestamp_ns()

        try:
            dt = datetime.fromisoformat(ts_str.replace("Z", "+00:00"))
            return dt_to_unix_nanos(dt)
        except Exception:
            return self._clock.timestamp_ns()

    def _parse_timestamp_ms(self, ts_ms: int | float) -> int:
        """Parse millisecond timestamp to nanoseconds."""
        if not ts_ms:
            return self._clock.timestamp_ns()
        return int(ts_ms * 1_000_000)  # ms to ns

    # -- Subscriptions ----

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        """Subscribe to quote ticks for an instrument."""
        symbol = command.instrument_id.symbol.value
        if symbol in self._subscribed_quote_symbols:
            return

        channel = f"{REDIS_QUOTE_CHANNEL_PREFIX}{symbol}"
        if self._pubsub:
            await self._pubsub.subscribe(channel)
            self._subscribed_quote_symbols.add(symbol)
            self._log.debug(f"Subscribed to quotes for {symbol}")

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        """Subscribe to trade ticks for an instrument."""
        # Trade ticks come from the same quote channel
        symbol = command.instrument_id.symbol.value
        if symbol in self._subscribed_quote_symbols:
            return

        channel = f"{REDIS_QUOTE_CHANNEL_PREFIX}{symbol}"
        if self._pubsub:
            await self._pubsub.subscribe(channel)
            self._subscribed_quote_symbols.add(symbol)
            self._log.debug(f"Subscribed to trades for {symbol}")

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        """Subscribe to bars for an instrument."""
        symbol = command.bar_type.instrument_id.symbol.value
        if symbol in self._subscribed_bar_symbols:
            return

        # Store the bar type for this symbol
        self._bar_types[symbol] = command.bar_type

        channel = f"{REDIS_BAR_CHANNEL_PREFIX}{symbol}"
        if self._pubsub:
            await self._pubsub.subscribe(channel)
            self._subscribed_bar_symbols.add(symbol)
            self._log.debug(f"Subscribed to bars for {symbol}")

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        """Unsubscribe from quote ticks for an instrument."""
        symbol = command.instrument_id.symbol.value
        if symbol not in self._subscribed_quote_symbols:
            return

        channel = f"{REDIS_QUOTE_CHANNEL_PREFIX}{symbol}"
        if self._pubsub:
            await self._pubsub.unsubscribe(channel)
            self._subscribed_quote_symbols.discard(symbol)

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        """Unsubscribe from trade ticks for an instrument."""
        symbol = command.instrument_id.symbol.value
        if symbol not in self._subscribed_quote_symbols:
            return

        channel = f"{REDIS_QUOTE_CHANNEL_PREFIX}{symbol}"
        if self._pubsub:
            await self._pubsub.unsubscribe(channel)
            self._subscribed_quote_symbols.discard(symbol)

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        """Unsubscribe from bars for an instrument."""
        symbol = command.bar_type.instrument_id.symbol.value
        if symbol not in self._subscribed_bar_symbols:
            return

        channel = f"{REDIS_BAR_CHANNEL_PREFIX}{symbol}"
        if self._pubsub:
            await self._pubsub.unsubscribe(channel)
            self._subscribed_bar_symbols.discard(symbol)
            self._bar_types.pop(symbol, None)

