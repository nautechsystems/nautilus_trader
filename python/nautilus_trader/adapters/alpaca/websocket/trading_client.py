# -------------------------------------------------------------------------------------------------
#  Bot-folio Alpaca Adapter for Nautilus Trader
#  https://github.com/mandeltechnologies/bot-folio
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import asyncio
import json
from typing import Any, Callable

import aiohttp

from nautilus_trader.adapters.alpaca.constants import get_trading_ws_url
from nautilus_trader.common.component import Logger


class AlpacaTradingWebSocketClient:
    """
    WebSocket client for Alpaca trading/order updates streaming.

    Connects to wss://paper-api.alpaca.markets/stream (or live equivalent)

    Parameters
    ----------
    api_key : str, optional
        The Alpaca API key.
    api_secret : str, optional
        The Alpaca API secret.
    access_token : str, optional
        The Alpaca OAuth access token.
    paper : bool, default True
        If using paper trading endpoints.
    logger : Logger, optional
        The logger for the client.

    """

    def __init__(
        self,
        api_key: str | None = None,
        api_secret: str | None = None,
        access_token: str | None = None,
        paper: bool = True,
        logger: Logger | None = None,
    ) -> None:
        self._api_key = api_key
        self._api_secret = api_secret
        self._access_token = access_token
        self._paper = paper
        self._logger = logger

        self._ws_url = get_trading_ws_url(paper)
        self._ws: aiohttp.ClientWebSocketResponse | None = None
        self._session: aiohttp.ClientSession | None = None
        self._running = False
        self._task: asyncio.Task | None = None

        # Callbacks for different order events
        self._on_trade_update: Callable[[dict[str, Any]], None] | None = None
        self._on_error: Callable[[str], None] | None = None

    def set_on_trade_update(self, callback: Callable[[dict[str, Any]], None]) -> None:
        """Set callback for trade update messages (order fills, cancels, etc.)."""
        self._on_trade_update = callback

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
            self._logger.debug(f"Alpaca trading WS auth response: {response}")

        # Check for auth success
        if response.get("stream") == "authorization":
            if response.get("data", {}).get("status") != "authorized":
                raise RuntimeError(
                    f"Alpaca trading WS auth failed: {response.get('data', {}).get('message')}"
                )

        # Subscribe to trade updates
        await self._subscribe_trade_updates()

        self._running = True
        self._task = asyncio.create_task(self._listen())

        if self._logger:
            self._logger.info("Alpaca trading WebSocket connected")

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

        if self._logger:
            self._logger.info("Alpaca trading WebSocket disconnected")

    def _build_auth_message(self) -> dict[str, Any]:
        """Build authentication message using Alpaca's current format."""
        if self._access_token:
            return {
                "action": "auth",
                "data": {"oauth_token": self._access_token},
            }
        # Use new format: {"action": "auth", "key": "...", "secret": "..."}
        return {
            "action": "auth",
            "key": self._api_key,
            "secret": self._api_secret,
        }

    async def _subscribe_trade_updates(self) -> None:
        """Subscribe to trade updates stream."""
        if not self._ws:
            return

        await self._ws.send_json({
            "action": "listen",
            "data": {"streams": ["trade_updates"]},
        })

        if self._logger:
            self._logger.debug("Subscribed to trade_updates stream")

    async def _listen(self) -> None:
        """Listen for incoming messages."""
        while self._running and self._ws:
            try:
                msg = await self._ws.receive()

                if msg.type == aiohttp.WSMsgType.TEXT:
                    data = json.loads(msg.data)
                    await self._handle_message(data)
                elif msg.type == aiohttp.WSMsgType.BINARY:
                    data = json.loads(msg.data.decode("utf-8"))
                    await self._handle_message(data)
                elif msg.type == aiohttp.WSMsgType.CLOSED:
                    if self._logger:
                        self._logger.warning("Alpaca trading WebSocket closed")
                    break
                elif msg.type == aiohttp.WSMsgType.ERROR:
                    if self._logger:
                        self._logger.error(f"Alpaca trading WebSocket error: {msg.data}")
                    if self._on_error:
                        self._on_error(str(msg.data))
                    break

            except asyncio.CancelledError:
                break
            except Exception as e:
                if self._logger:
                    self._logger.error(f"Alpaca trading WS listen error: {e}")
                if self._on_error:
                    self._on_error(str(e))

    async def _handle_message(self, data: dict[str, Any]) -> None:
        """Handle incoming WebSocket message."""
        stream = data.get("stream")

        if stream == "trade_updates":
            if self._on_trade_update:
                self._on_trade_update(data.get("data", {}))
        elif stream == "listening":
            if self._logger:
                self._logger.debug(f"Alpaca trading WS listening: {data.get('data')}")
        elif stream == "authorization":
            # Already handled in connect
            pass
        else:
            if self._logger:
                self._logger.debug(f"Alpaca trading WS unknown message: {data}")

