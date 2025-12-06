# -------------------------------------------------------------------------------------------------
#  Bot-folio Alpaca Adapter for Nautilus Trader
#  https://github.com/mandeltechnologies/bot-folio
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import asyncio
from typing import Any
from urllib.parse import quote

import aiohttp

from nautilus_trader.adapters.alpaca.constants import ALPACA_DATA_API_URL
from nautilus_trader.adapters.alpaca.constants import get_trading_api_url
from nautilus_trader.adapters.alpaca.credentials import get_auth_headers
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger


class AlpacaHttpClient:
    """
    HTTP client for Alpaca REST API.

    Parameters
    ----------
    clock : LiveClock
        The clock for the client.
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
        clock: LiveClock,
        api_key: str | None = None,
        api_secret: str | None = None,
        access_token: str | None = None,
        paper: bool = True,
        logger: Logger | None = None,
    ) -> None:
        self._clock = clock
        self._api_key = api_key
        self._api_secret = api_secret
        self._access_token = access_token
        self._paper = paper
        self._logger = logger

        self._trading_base_url = get_trading_api_url(paper)
        self._data_base_url = ALPACA_DATA_API_URL
        self._auth_headers = get_auth_headers(api_key, api_secret, access_token)

        self._session: aiohttp.ClientSession | None = None

    @property
    def trading_base_url(self) -> str:
        """Return the trading API base URL."""
        return self._trading_base_url

    @property
    def data_base_url(self) -> str:
        """Return the data API base URL."""
        return self._data_base_url

    async def connect(self) -> None:
        """Connect the HTTP client (create session)."""
        if self._session is None:
            self._session = aiohttp.ClientSession(
                headers=self._auth_headers,
                timeout=aiohttp.ClientTimeout(total=30),
            )
            if self._logger:
                self._logger.info("Alpaca HTTP client connected")

    async def disconnect(self) -> None:
        """Disconnect the HTTP client (close session)."""
        if self._session:
            await self._session.close()
            self._session = None
            if self._logger:
                self._logger.info("Alpaca HTTP client disconnected")

    async def _request(
        self,
        method: str,
        url: str,
        params: dict[str, Any] | None = None,
        json: dict[str, Any] | None = None,
    ) -> dict[str, Any] | list[dict[str, Any]]:
        """
        Make an HTTP request.

        Parameters
        ----------
        method : str
            The HTTP method (GET, POST, DELETE, PATCH).
        url : str
            The full URL.
        params : dict, optional
            Query parameters.
        json : dict, optional
            JSON body for POST/PATCH.

        Returns
        -------
        dict or list
            The JSON response.

        Raises
        ------
        RuntimeError
            If client is not connected.
        aiohttp.ClientResponseError
            If the request fails.

        """
        if self._session is None:
            raise RuntimeError("AlpacaHttpClient not connected. Call connect() first.")

        async with self._session.request(
            method=method,
            url=url,
            params=params,
            json=json,
        ) as response:
            response.raise_for_status()
            return await response.json()

    # ---- Trading API Methods ----

    async def get_account(self) -> dict[str, Any]:
        """Get account information."""
        return await self._request("GET", f"{self._trading_base_url}/v2/account")

    async def get_positions(self) -> list[dict[str, Any]]:
        """Get all open positions."""
        return await self._request("GET", f"{self._trading_base_url}/v2/positions")

    async def get_position(self, symbol: str) -> dict[str, Any]:
        """Get position for a specific symbol."""
        return await self._request("GET", f"{self._trading_base_url}/v2/positions/{symbol}")

    async def get_orders(
        self,
        status: str = "open",
        limit: int = 500,
        after: str | None = None,
        until: str | None = None,
        direction: str = "desc",
        nested: bool = False,
        symbols: list[str] | None = None,
    ) -> list[dict[str, Any]]:
        """Get orders with optional filters."""
        params: dict[str, Any] = {
            "status": status,
            "limit": limit,
            "direction": direction,
            "nested": "true" if nested else "false",
        }
        if after:
            params["after"] = after
        if until:
            params["until"] = until
        if symbols:
            params["symbols"] = ",".join(symbols)

        return await self._request("GET", f"{self._trading_base_url}/v2/orders", params=params)

    async def get_order(self, order_id: str) -> dict[str, Any]:
        """Get a specific order by ID."""
        return await self._request("GET", f"{self._trading_base_url}/v2/orders/{order_id}")

    async def get_order_by_client_id(self, client_order_id: str) -> dict[str, Any]:
        """Get a specific order by client order ID."""
        return await self._request(
            "GET",
            f"{self._trading_base_url}/v2/orders:by_client_order_id",
            params={"client_order_id": client_order_id},
        )

    async def submit_order(
        self,
        symbol: str,
        qty: str | None = None,
        notional: str | None = None,
        side: str = "buy",
        type: str = "market",
        time_in_force: str = "day",
        limit_price: str | None = None,
        stop_price: str | None = None,
        client_order_id: str | None = None,
        extended_hours: bool = False,
        order_class: str | None = None,
        take_profit: dict[str, str] | None = None,
        stop_loss: dict[str, str] | None = None,
        trail_price: str | None = None,
        trail_percent: str | None = None,
    ) -> dict[str, Any]:
        """Submit a new order."""
        body: dict[str, Any] = {
            "symbol": symbol,
            "side": side,
            "type": type,
            "time_in_force": time_in_force,
        }

        if qty:
            body["qty"] = qty
        if notional:
            body["notional"] = notional
        if limit_price:
            body["limit_price"] = limit_price
        if stop_price:
            body["stop_price"] = stop_price
        if client_order_id:
            body["client_order_id"] = client_order_id
        if extended_hours:
            body["extended_hours"] = extended_hours
        if order_class:
            body["order_class"] = order_class
        if take_profit:
            body["take_profit"] = take_profit
        if stop_loss:
            body["stop_loss"] = stop_loss
        if trail_price:
            body["trail_price"] = trail_price
        if trail_percent:
            body["trail_percent"] = trail_percent

        return await self._request("POST", f"{self._trading_base_url}/v2/orders", json=body)

    async def cancel_order(self, order_id: str) -> None:
        """Cancel an order by ID."""
        await self._request("DELETE", f"{self._trading_base_url}/v2/orders/{order_id}")

    async def cancel_all_orders(self) -> list[dict[str, Any]]:
        """Cancel all open orders."""
        return await self._request("DELETE", f"{self._trading_base_url}/v2/orders")

    async def replace_order(
        self,
        order_id: str,
        qty: str | None = None,
        limit_price: str | None = None,
        stop_price: str | None = None,
        time_in_force: str | None = None,
        client_order_id: str | None = None,
    ) -> dict[str, Any]:
        """Replace/modify an existing order."""
        body: dict[str, Any] = {}

        if qty:
            body["qty"] = qty
        if limit_price:
            body["limit_price"] = limit_price
        if stop_price:
            body["stop_price"] = stop_price
        if time_in_force:
            body["time_in_force"] = time_in_force
        if client_order_id:
            body["client_order_id"] = client_order_id

        return await self._request(
            "PATCH", f"{self._trading_base_url}/v2/orders/{order_id}", json=body
        )

    # ---- Assets API Methods ----

    async def get_assets(
        self,
        status: str | None = None,
        asset_class: str | None = None,
        exchange: str | None = None,
    ) -> list[dict[str, Any]]:
        """Get list of assets."""
        params: dict[str, Any] = {}
        if status:
            params["status"] = status
        if asset_class:
            params["asset_class"] = asset_class
        if exchange:
            params["exchange"] = exchange

        return await self._request("GET", f"{self._trading_base_url}/v2/assets", params=params)

    async def get_asset(self, symbol_or_id: str) -> dict[str, Any]:
        """Get a specific asset by symbol or ID."""
        # URL-encode the symbol to handle crypto pairs with "/" (e.g., "BTC/USD" -> "BTC%2FUSD")
        encoded_symbol = quote(symbol_or_id, safe="")
        return await self._request("GET", f"{self._trading_base_url}/v2/assets/{encoded_symbol}")

    # ---- Data API Methods ----

    def _is_crypto_symbol(self, symbol: str) -> bool:
        """Check if a symbol is a crypto pair (e.g., BTC/USD)."""
        return "/" in symbol

    def _get_data_endpoint(self, symbol: str, data_type: str) -> str:
        """Get the appropriate data endpoint for a symbol.
        
        Parameters
        ----------
        symbol : str
            The symbol (e.g., "AAPL" for stocks, "BTC/USD" for crypto).
        data_type : str
            The data type: "bars", "quotes", "trades".
            
        Returns
        -------
        str
            The full endpoint URL.
        """
        if self._is_crypto_symbol(symbol):
            # Crypto uses v1beta3 endpoint
            return f"{self._data_base_url}/v1beta3/crypto/us/{data_type}"
        else:
            # Stocks use v2 endpoint
            return f"{self._data_base_url}/v2/stocks/{symbol}/{data_type}"

    async def get_bars(
        self,
        symbol: str,
        timeframe: str = "1Day",
        start: str | None = None,
        end: str | None = None,
        limit: int = 1000,
        adjustment: str = "raw",
        feed: str = "iex",
    ) -> dict[str, Any]:
        """Get historical bars for a symbol."""
        is_crypto = self._is_crypto_symbol(symbol)
        
        params: dict[str, Any] = {
            "timeframe": timeframe,
            "limit": limit,
        }
        
        if is_crypto:
            # Crypto endpoint uses "symbols" parameter
            params["symbols"] = symbol
        else:
            # Stocks endpoint uses path parameter and additional options
            params["adjustment"] = adjustment
            params["feed"] = feed
            
        if start:
            params["start"] = start
        if end:
            params["end"] = end

        endpoint = self._get_data_endpoint(symbol, "bars")
        return await self._request("GET", endpoint, params=params)

    async def get_quotes(
        self,
        symbol: str,
        start: str | None = None,
        end: str | None = None,
        limit: int = 1000,
        feed: str = "iex",
    ) -> dict[str, Any]:
        """Get historical quotes for a symbol."""
        is_crypto = self._is_crypto_symbol(symbol)
        
        params: dict[str, Any] = {
            "limit": limit,
        }
        
        if is_crypto:
            params["symbols"] = symbol
        else:
            params["feed"] = feed
            
        if start:
            params["start"] = start
        if end:
            params["end"] = end

        endpoint = self._get_data_endpoint(symbol, "quotes")
        return await self._request("GET", endpoint, params=params)

    async def get_trades(
        self,
        symbol: str,
        start: str | None = None,
        end: str | None = None,
        limit: int = 1000,
        feed: str = "iex",
    ) -> dict[str, Any]:
        """Get historical trades for a symbol."""
        is_crypto = self._is_crypto_symbol(symbol)
        
        params: dict[str, Any] = {
            "limit": limit,
        }
        
        if is_crypto:
            params["symbols"] = symbol
        else:
            params["feed"] = feed
            
        if start:
            params["start"] = start
        if end:
            params["end"] = end

        endpoint = self._get_data_endpoint(symbol, "trades")
        return await self._request("GET", endpoint, params=params)

    async def get_latest_quote(self, symbol: str, feed: str = "iex") -> dict[str, Any]:
        """Get latest quote for a symbol."""
        is_crypto = self._is_crypto_symbol(symbol)
        
        if is_crypto:
            return await self._request(
                "GET",
                f"{self._data_base_url}/v1beta3/crypto/us/latest/quotes",
                params={"symbols": symbol},
            )
        else:
            return await self._request(
                "GET",
                f"{self._data_base_url}/v2/stocks/{symbol}/quotes/latest",
                params={"feed": feed},
            )

    async def get_latest_trade(self, symbol: str, feed: str = "iex") -> dict[str, Any]:
        """Get latest trade for a symbol."""
        is_crypto = self._is_crypto_symbol(symbol)
        
        if is_crypto:
            return await self._request(
                "GET",
                f"{self._data_base_url}/v1beta3/crypto/us/latest/trades",
                params={"symbols": symbol},
            )
        else:
            return await self._request(
                "GET",
                f"{self._data_base_url}/v2/stocks/{symbol}/trades/latest",
                params={"feed": feed},
            )

