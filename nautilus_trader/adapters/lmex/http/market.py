# -------------------------------------------------------------------------------------------------
#  LMEX NautilusTrader Adapter
#  Licensed under the GNU Lesser General Public License Version 3.0
# -------------------------------------------------------------------------------------------------

"""
LMEX REST market-data endpoint wrappers.

All methods return typed objects decoded via ``msgspec``.  None of these
endpoints require authentication.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

import msgspec

from nautilus_trader.adapters.lmex.schemas.market import LmexMarketSummary
from nautilus_trader.adapters.lmex.schemas.market import LmexOrderBook
from nautilus_trader.adapters.lmex.schemas.market import LmexServerTime
from nautilus_trader.adapters.lmex.schemas.market import LmexTrade

if TYPE_CHECKING:
    from nautilus_trader.adapters.lmex.http.client import LmexHttpClient


class LmexMarketHttpAPI:
    """
    Provides typed wrappers around the LMEX public market-data REST endpoints.

    Parameters
    ----------
    client : LmexHttpClient
        The underlying authenticated HTTP client.

    """

    # msgspec decoders (created once, reused for performance)
    _dec_server_time = msgspec.json.Decoder(LmexServerTime)
    _dec_orderbook = msgspec.json.Decoder(LmexOrderBook)
    _dec_trades = msgspec.json.Decoder(list[LmexTrade])
    _dec_market_summary = msgspec.json.Decoder(list[LmexMarketSummary])

    def __init__(self, client: LmexHttpClient) -> None:
        self._client = client

    async def get_server_time(self) -> LmexServerTime:
        """
        Fetch the current server time.

        Endpoint
        --------
        ``GET /api/v3.2/time``

        Returns
        -------
        LmexServerTime

        """
        raw = await self._client.get("/api/v3.2/time")
        return self._dec_server_time.decode(raw)

    async def get_orderbook(
        self,
        symbol: str,
        depth: int | None = None,
    ) -> LmexOrderBook:
        """
        Fetch the current order book snapshot for a symbol.

        Endpoint
        --------
        ``GET /api/v3.2/orderbook``

        Parameters
        ----------
        symbol : str
            The trading pair (e.g. ``"BTC-USD"``).
        depth : int, optional
            Number of price levels to return on each side.
            If ``None`` the exchange default is used (typically 20).

        Returns
        -------
        LmexOrderBook

        """
        params: dict[str, str | int] = {"symbol": symbol}
        if depth is not None:
            params["depth"] = depth
        raw = await self._client.get("/api/v3.2/orderbook", params=params)
        return self._dec_orderbook.decode(raw)

    async def get_trades(
        self,
        symbol: str,
        count: int | None = None,
    ) -> list[LmexTrade]:
        """
        Fetch recent public trades for a symbol.

        Endpoint
        --------
        ``GET /api/v3.2/trades``

        Parameters
        ----------
        symbol : str
            The trading pair (e.g. ``"BTC-USD"``).
        count : int, optional
            Number of trades to return.  Defaults to exchange maximum.

        Returns
        -------
        list[LmexTrade]

        """
        params: dict[str, str | int] = {"symbol": symbol}
        if count is not None:
            params["count"] = count
        raw = await self._client.get("/api/v3.2/trades", params=params)
        return self._dec_trades.decode(raw)

    async def get_market_summary(
        self,
        symbol: str | None = None,
    ) -> list[LmexMarketSummary]:
        """
        Fetch instrument metadata for one or all trading pairs.

        This is the primary source for instrument definitions.

        Endpoint
        --------
        ``GET /api/v3.2/market_summary``

        Parameters
        ----------
        symbol : str, optional
            When provided, returns only the entry for that symbol.
            When ``None``, returns all instruments (2,000+ entries).

        Returns
        -------
        list[LmexMarketSummary]
            Always a list; single-symbol requests still return a one-element
            list.

        """
        params: dict[str, str] | None = {"symbol": symbol} if symbol else None
        raw = await self._client.get("/api/v3.2/market_summary", params=params)
        return self._dec_market_summary.decode(raw)
