# -------------------------------------------------------------------------------------------------
#  Bot-folio Alpaca Adapter for Nautilus Trader
#  https://github.com/mandeltechnologies/bot-folio
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import asyncio
import json
from typing import Any, Callable

import aiohttp

from nautilus_trader.adapters.alpaca.constants import get_data_ws_url
from nautilus_trader.adapters.alpaca.credentials import get_auth_headers
from nautilus_trader.common.component import Logger


class AlpacaDataWebSocketClient:
    """
    WebSocket client for Alpaca market data streaming.

    Connects to wss://stream.data.alpaca.markets/v2/{feed}

    Parameters
    ----------
    api_key : str, optional
        The Alpaca API key.
    api_secret : str, optional
        The Alpaca API secret.
    access_token : str, optional
        The Alpaca OAuth access token.
    feed : str, default "iex"
        The data feed: "iex" (free) or "sip" (paid).
    logger : Logger, optional
        The logger for the client.

    """

    def __init__(
        self,
        api_key: str | None = None,
        api_secret: str | None = None,
        access_token: str | None = None,
        feed: str = "iex",
        logger: Logger | None = None,
    ) -> None:
        self._api_key = api_key
        self._api_secret = api_secret
        self._access_token = access_token
        self._feed = feed
        self._logger = logger

        self._ws_url = get_data_ws_url(feed)
        self._ws: aiohttp.ClientWebSocketResponse | None = None
        self._session: aiohttp.ClientSession | None = None
        self._running = False
        self._task: asyncio.Task | None = None

        # Callbacks for different message types
        self._on_quote: Callable[[dict[str, Any]], None] | None = None
        self._on_trade: Callable[[dict[str, Any]], None] | None = None
        self._on_bar: Callable[[dict[str, Any]], None] | None = None
        self._on_error: Callable[[str], None] | None = None

        # Subscriptions tracking
        self._subscribed_quotes: set[str] = set()
        self._subscribed_trades: set[str] = set()
        self._subscribed_bars: set[str] = set()

    def set_on_quote(self, callback: Callable[[dict[str, Any]], None]) -> None:
        """Set callback for quote messages."""
        self._on_quote = callback

    def set_on_trade(self, callback: Callable[[dict[str, Any]], None]) -> None:
        """Set callback for trade messages."""
        self._on_trade = callback

    def set_on_bar(self, callback: Callable[[dict[str, Any]], None]) -> None:
        """Set callback for bar messages."""
        self._on_bar = callback

    def set_on_error(self, callback: Callable[[str], None]) -> None:
        """Set callback for error messages."""
        self._on_error = callback

    async def connect(self) -> None:
        """Connect to the WebSocket and authenticate."""
        if self._ws is not None:
            return

        self._session = aiohttp.ClientSession()
        self._ws = await self._session.ws_connect(self._ws_url)

        # Authenticate
        auth_msg = self._build_auth_message()
        await self._ws.send_json(auth_msg)

        # Wait for auth response (handle both TEXT and BINARY messages)
        msg = await self._ws.receive()
        if msg.type == aiohttp.WSMsgType.TEXT:
            response = json.loads(msg.data)
        elif msg.type == aiohttp.WSMsgType.BINARY:
            response = json.loads(msg.data.decode("utf-8"))
        else:
            raise RuntimeError(f"Unexpected WS message type during auth: {msg.type}")

        if self._logger:
            self._logger.debug(f"Alpaca data WS auth response: {response}")

        # Check for auth success
        for msg in response if isinstance(response, list) else [response]:
            if msg.get("T") == "error":
                raise RuntimeError(f"Alpaca data WS auth failed: {msg.get('msg')}")

        self._running = True
        self._task = asyncio.create_task(self._listen())

        if self._logger:
            self._logger.info(f"Alpaca data WebSocket connected to {self._feed} feed")

    async def disconnect(self) -> None:
        """Disconnect from the WebSocket."""
        self._running = False

        if self._task:
            self._task.cancel()
            try:
                await self._task
            except asyncio.CancelledError:
                pass
            self._task = None

        if self._ws:
            await self._ws.close()
            self._ws = None

        if self._session:
            await self._session.close()
            self._session = None

        self._subscribed_quotes.clear()
        self._subscribed_trades.clear()
        self._subscribed_bars.clear()

        if self._logger:
            self._logger.info("Alpaca data WebSocket disconnected")

    def _build_auth_message(self) -> dict[str, Any]:
        """Build authentication message."""
        if self._access_token:
            return {"action": "auth", "oauth_token": self._access_token}
        return {
            "action": "auth",
            "key": self._api_key,
            "secret": self._api_secret,
        }

    async def _listen(self) -> None:
        """Listen for incoming messages."""
        while self._running and self._ws:
            try:
                msg = await self._ws.receive()

                if msg.type == aiohttp.WSMsgType.TEXT:
                    data = json.loads(msg.data)
                    await self._handle_messages(data)
                elif msg.type == aiohttp.WSMsgType.BINARY:
                    data = json.loads(msg.data.decode("utf-8"))
                    await self._handle_messages(data)
                elif msg.type == aiohttp.WSMsgType.CLOSED:
                    if self._logger:
                        self._logger.warning("Alpaca data WebSocket closed")
                    break
                elif msg.type == aiohttp.WSMsgType.ERROR:
                    if self._logger:
                        self._logger.error(f"Alpaca data WebSocket error: {msg.data}")
                    if self._on_error:
                        self._on_error(str(msg.data))
                    break

            except asyncio.CancelledError:
                break
            except Exception as e:
                if self._logger:
                    self._logger.error(f"Alpaca data WS listen error: {e}")
                if self._on_error:
                    self._on_error(str(e))

    async def _handle_messages(self, data: list[dict[str, Any]] | dict[str, Any]) -> None:
        """Handle incoming WebSocket messages."""
        messages = data if isinstance(data, list) else [data]

        for msg in messages:
            msg_type = msg.get("T")

            if msg_type == "q" and self._on_quote:
                self._on_quote(msg)
            elif msg_type == "t" and self._on_trade:
                self._on_trade(msg)
            elif msg_type == "b" and self._on_bar:
                self._on_bar(msg)
            elif msg_type == "error":
                if self._logger:
                    self._logger.error(f"Alpaca data WS error: {msg.get('msg')}")
                if self._on_error:
                    self._on_error(msg.get("msg", "Unknown error"))
            elif msg_type == "subscription":
                if self._logger:
                    self._logger.debug(f"Alpaca subscription update: {msg}")

    async def subscribe_quotes(self, symbols: list[str]) -> None:
        """Subscribe to quote updates for symbols."""
        if not self._ws:
            raise RuntimeError("WebSocket not connected")

        new_symbols = [s for s in symbols if s not in self._subscribed_quotes]
        if not new_symbols:
            return

        await self._ws.send_json({"action": "subscribe", "quotes": new_symbols})
        self._subscribed_quotes.update(new_symbols)

        if self._logger:
            self._logger.debug(f"Subscribed to quotes: {new_symbols}")

    async def subscribe_trades(self, symbols: list[str]) -> None:
        """Subscribe to trade updates for symbols."""
        if not self._ws:
            raise RuntimeError("WebSocket not connected")

        new_symbols = [s for s in symbols if s not in self._subscribed_trades]
        if not new_symbols:
            return

        await self._ws.send_json({"action": "subscribe", "trades": new_symbols})
        self._subscribed_trades.update(new_symbols)

        if self._logger:
            self._logger.debug(f"Subscribed to trades: {new_symbols}")

    async def subscribe_bars(self, symbols: list[str]) -> None:
        """Subscribe to bar updates for symbols."""
        if not self._ws:
            raise RuntimeError("WebSocket not connected")

        new_symbols = [s for s in symbols if s not in self._subscribed_bars]
        if not new_symbols:
            return

        await self._ws.send_json({"action": "subscribe", "bars": new_symbols})
        self._subscribed_bars.update(new_symbols)

        if self._logger:
            self._logger.debug(f"Subscribed to bars: {new_symbols}")

    async def unsubscribe_quotes(self, symbols: list[str]) -> None:
        """Unsubscribe from quote updates for symbols."""
        if not self._ws:
            return

        to_unsub = [s for s in symbols if s in self._subscribed_quotes]
        if not to_unsub:
            return

        await self._ws.send_json({"action": "unsubscribe", "quotes": to_unsub})
        self._subscribed_quotes.difference_update(to_unsub)

    async def unsubscribe_trades(self, symbols: list[str]) -> None:
        """Unsubscribe from trade updates for symbols."""
        if not self._ws:
            return

        to_unsub = [s for s in symbols if s in self._subscribed_trades]
        if not to_unsub:
            return

        await self._ws.send_json({"action": "unsubscribe", "trades": to_unsub})
        self._subscribed_trades.difference_update(to_unsub)

    async def unsubscribe_bars(self, symbols: list[str]) -> None:
        """Unsubscribe from bar updates for symbols."""
        if not self._ws:
            return

        to_unsub = [s for s in symbols if s in self._subscribed_bars]
        if not to_unsub:
            return

        await self._ws.send_json({"action": "unsubscribe", "bars": to_unsub})
        self._subscribed_bars.difference_update(to_unsub)

