# -------------------------------------------------------------------------------------------------
#  LMEX NautilusTrader Adapter
#  Licensed under the GNU Lesser General Public License Version 3.0
# -------------------------------------------------------------------------------------------------

"""
LMEX REST account/trading endpoint wrappers.

All methods in this module require authentication (``signed=True`` is set
automatically by the ``post`` / ``delete`` helpers on ``LmexHttpClient``).

Endpoint facts verified against ``https://test-api.lmex.io/spot`` on 2026-05-26:

- ``POST /api/v3.2/order``         → list[LmexOrderResponse]  (JSON body)
- ``DELETE /api/v3.2/order``       → list[LmexOrderResponse]  (query params)
- ``GET /api/v3.2/user/open_orders``   → list[LmexOpenOrder]
- ``GET /api/v3.2/user/trade_history`` → list[LmexFill]
- ``GET /api/v3.2/user/wallet``        → list[LmexWalletEntry]
- No bulk-cancel endpoint exists; cancellation is per-order.
"""

from __future__ import annotations

from typing import TYPE_CHECKING, Any

import msgspec

from nautilus_trader.adapters.lmex.schemas.order import LmexFill
from nautilus_trader.adapters.lmex.schemas.order import LmexOpenOrder
from nautilus_trader.adapters.lmex.schemas.order import LmexOrderResponse
from nautilus_trader.adapters.lmex.schemas.order import LmexWalletEntry

if TYPE_CHECKING:
    from nautilus_trader.adapters.lmex.http.client import LmexHttpClient


class LmexAccountHttpAPI:
    """
    Provides typed wrappers around the LMEX authenticated trading endpoints.

    Parameters
    ----------
    client : LmexHttpClient
        The underlying HTTP client (must have api_key + api_secret configured).

    """

    # POST /order and DELETE /order both return list[LmexOrderResponse]
    _dec_order_response = msgspec.json.Decoder(list[LmexOrderResponse])
    _dec_open_orders = msgspec.json.Decoder(list[LmexOpenOrder])
    _dec_fills = msgspec.json.Decoder(list[LmexFill])
    _dec_wallet = msgspec.json.Decoder(list[LmexWalletEntry])

    def __init__(self, client: LmexHttpClient) -> None:
        self._client = client

    async def get_open_orders(
        self,
        symbol: str | None = None,
    ) -> list[LmexOpenOrder]:
        """
        Fetch all open orders, optionally filtered by symbol.

        Endpoint: ``GET /api/v3.2/user/open_orders``

        Parameters
        ----------
        symbol : str, optional
            Filter to a specific trading pair.  When ``None`` all open orders
            across all symbols are returned.

        Returns
        -------
        list[LmexOpenOrder]

        """
        params: dict[str, str] | None = {"symbol": symbol} if symbol else None
        raw = await self._client.get(
            "/api/v3.2/user/open_orders",
            params=params,
            signed=True,
        )
        return self._dec_open_orders.decode(raw)

    async def submit_order(
        self,
        symbol: str,
        side: str,
        order_type: str,
        size: float,
        price: float | None = None,
        client_order_id: str | None = None,
        time_in_force: str | None = None,
        post_only: bool = False,
        reduce_only: bool = False,
    ) -> LmexOrderResponse:
        """
        Submit a new order.

        Endpoint: ``POST /api/v3.2/order``

        The exchange returns a **list**; this method returns the first element.

        Parameters
        ----------
        symbol : str
            Trading pair (e.g. ``"BTC-USD"``).
        side : str
            ``"BUY"`` or ``"SELL"``.
        order_type : str
            ``"LIMIT"``, ``"MARKET"``, etc.
        size : float
            Order quantity in base currency.
        price : float, optional
            Limit price (required for LIMIT orders).
        client_order_id : str, optional
            Client-assigned identifier echoed back in responses and WS events.
        time_in_force : str, optional
            ``"GTC"``, ``"IOC"``, ``"FOK"``, etc.
        post_only : bool, default False
            When ``True`` the order is rejected if it would execute immediately.
        reduce_only : bool, default False
            When ``True`` the order may only reduce an existing position.

        Returns
        -------
        LmexOrderResponse
            The first (and only) element of the exchange response list.

        """
        payload: dict[str, Any] = {
            "symbol": symbol,
            "side": side,
            "type": order_type,
            "size": size,
        }
        if price is not None:
            payload["price"] = price
        if client_order_id is not None:
            payload["clOrderId"] = client_order_id
        if time_in_force is not None:
            payload["time_in_force"] = time_in_force
        if post_only:
            payload["postOnly"] = True
        if reduce_only:
            payload["reduceOnly"] = True

        raw = await self._client.post("/api/v3.2/order", payload=payload)
        results = self._dec_order_response.decode(raw)
        return results[0]

    async def cancel_order(
        self,
        symbol: str,
        order_id: str,
        client_order_id: str | None = None,
    ) -> LmexOrderResponse:
        """
        Cancel an existing order by exchange UUID or client order ID.

        Endpoint: ``DELETE /api/v3.2/order`` (query params)

        Parameters
        ----------
        symbol : str
            Trading pair.
        order_id : str
            Exchange-assigned UUID order ID.  Takes precedence over
            ``client_order_id`` when non-empty.
        client_order_id : str, optional
            Client order ID (used as fallback if ``order_id`` is unavailable).

        Returns
        -------
        LmexOrderResponse
            The cancel acknowledgement (status field will be 6 = CANCELLED).

        """
        params: dict[str, str] = {"symbol": symbol}
        if order_id:
            params["orderID"] = order_id
        elif client_order_id:
            params["clOrderID"] = client_order_id
        else:
            raise ValueError("Either order_id or client_order_id must be provided")

        raw = await self._client.delete("/api/v3.2/order", params=params)
        results = self._dec_order_response.decode(raw)
        return results[0]

    async def cancel_all_orders(self, symbol: str) -> list[LmexOrderResponse]:
        """
        Cancel all open orders for a symbol.

        LMEX has no bulk-cancel endpoint.  This method fetches all open orders
        for ``symbol`` and cancels each one individually.

        Parameters
        ----------
        symbol : str
            Trading pair.

        Returns
        -------
        list[LmexOrderResponse]
            Cancel acknowledgements for each cancelled order.

        """
        open_orders = await self.get_open_orders(symbol=symbol)
        results: list[LmexOrderResponse] = []
        for order in open_orders:
            try:
                result = await self.cancel_order(symbol=symbol, order_id=order.orderID)
                results.append(result)
            except Exception:
                # Best-effort: continue cancelling remaining orders
                pass
        return results

    async def get_fills(
        self,
        symbol: str | None = None,
        start_time: int | None = None,
        end_time: int | None = None,
        count: int | None = None,
    ) -> list[LmexFill]:
        """
        Fetch trade fill / execution history.

        Endpoint: ``GET /api/v3.2/user/trade_history``

        Parameters
        ----------
        symbol : str, optional
            Filter to a specific trading pair.
        start_time : int, optional
            Start of time range (epoch milliseconds).
        end_time : int, optional
            End of time range (epoch milliseconds).
        count : int, optional
            Maximum number of records to return.

        Returns
        -------
        list[LmexFill]

        """
        params: dict[str, Any] = {}
        if symbol:
            params["symbol"] = symbol
        if start_time is not None:
            params["startTime"] = start_time
        if end_time is not None:
            params["endTime"] = end_time
        if count is not None:
            params["count"] = count

        raw = await self._client.get(
            "/api/v3.2/user/trade_history",
            params=params or None,
            signed=True,
        )
        return self._dec_fills.decode(raw)

    async def get_wallet_balance(self) -> list[LmexWalletEntry]:
        """
        Fetch account wallet balances for all currencies.

        Endpoint: ``GET /api/v3.2/user/wallet``

        Returns
        -------
        list[LmexWalletEntry]

        """
        raw = await self._client.get("/api/v3.2/user/wallet", signed=True)
        return self._dec_wallet.decode(raw)
