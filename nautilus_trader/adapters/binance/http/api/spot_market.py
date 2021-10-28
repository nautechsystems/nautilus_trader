# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
#
#  Heavily refactored from MIT licensed github.com/binance/binance-connector-python
#  Original author: Jeremy https://github.com/2pd
# -------------------------------------------------------------------------------------------------

from typing import Dict, Optional

from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.parsing import convert_list_to_json_array
from nautilus_trader.core.correctness import PyCondition


class BinanceSpotMarketHttpAPI:
    """
    Provides access to the `Binance SPOT Market` HTTP REST API.
    """

    BASE_ENDPOINT = "/api/v3/"

    def __init__(self, client: BinanceHttpClient):
        """
        Initialize a new instance of the ``BinanceSpotMarketHttpAPI`` class.

        Parameters
        ----------
        client : BinanceHttpClient
            The Binance REST API client.

        """
        PyCondition.not_none(client, "client")

        self.client = client

    async def ping(self) -> bytes:
        """
        Test the connectivity to the REST API.

        `GET /api/v3/ping`

        Returns
        -------
        bytes
            The raw response content.

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#test-connectivity

        """
        return await self.client.query(url_path=self.BASE_ENDPOINT + "ping")

    async def time(self) -> bytes:
        """
        Check Server Time.

        Test connectivity to the Rest API and get the current server time.

        `GET /api/v3/time`

        Returns
        -------
        bytes
            The raw response content.

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#check-server-time

        """
        return await self.client.query(url_path=self.BASE_ENDPOINT + "time")

    async def exchange_info(self, symbol: str = None, symbols: list = None) -> bytes:
        """
        Exchange Information.

        Current exchange trading rules and symbol information.
        Only either `symbol` or `symbols` should be passed.

        `GET /api/v3/exchangeinfo`

        Parameters
        ----------
        symbol : str, optional
            The trading pair.
        symbols : list[str], optional
            The list of trading pairs.

        Returns
        -------
        bytes
            The raw response content.

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#exchange-information

        """
        if symbol and symbols:
            raise ValueError("`symbol` and `symbols` cannot be sent together")

        payload: Dict[str, str] = {}
        if symbol is not None:
            payload["symbol"] = symbol
        if symbols is not None:
            payload["symbols"] = convert_list_to_json_array(symbols)

        return await self.client.query(
            url_path=self.BASE_ENDPOINT + "exchangeInfo",
            payload=payload,
        )

    async def depth(self, symbol: str, limit: Optional[int] = None) -> bytes:
        """
        Get orderbook.

        `GET /api/v3/depth`

        Parameters
        ----------
        symbol : str
            The trading pair.
        limit : int, optional, default 100
            The limit for the response. Default 100; max 5000.
            Valid limits:[5, 10, 20, 50, 100, 500, 1000, 5000].

        Returns
        -------
        bytes
            The raw response content.

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#order-book

        """
        payload: Dict[str, str] = {"symbol": symbol}
        if limit is not None:
            payload["limit"] = str(limit)

        return await self.client.query(
            url_path=self.BASE_ENDPOINT + "depth",
            payload=payload,
        )

    async def trades(self, symbol: str, limit: Optional[int] = None) -> bytes:
        """
        Recent Trades List.

        Get recent trades (up to last 500).

        `GET /api/v3/trades`

        Parameters
        ----------
        symbol : str
            The trading pair.
        limit : int, optional
            The limit for the response. Default 500; max 1000.

        Returns
        -------
        bytes
            The raw response content.

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#recent-trades-list

        """
        payload: Dict[str, str] = {"symbol": symbol}
        if limit is not None:
            payload["limit"] = str(limit)

        return await self.client.query(
            url_path=self.BASE_ENDPOINT + "trades",
            payload=payload,
        )

    async def historical_trades(
        self,
        symbol: str,
        from_id: Optional[int] = None,
        limit: Optional[int] = None,
    ) -> bytes:
        """
        Old Trade Lookup.

        Get older market trades.

        `GET /api/v3/historicalTrades`

        Parameters
        ----------
        symbol : str
            The trading pair.
        from_id : int, optional
            The trade ID to fetch from. Default gets most recent trades.
        limit : int, optional
            The limit for the response. Default 500; max 1000.

        Returns
        -------
        bytes
            The raw response content.

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#old-trade-lookup

        """
        payload: Dict[str, str] = {"symbol": symbol}
        if limit is not None:
            payload["limit"] = str(limit)
        if from_id is not None:
            payload["fromId"] = str(from_id)

        return await self.client.limit_request(
            http_method="GET",
            url_path=self.BASE_ENDPOINT + "historicalTrades",
            payload=payload,
        )

    async def agg_trades(
        self,
        symbol: str,
        from_id: Optional[int] = None,
        start_time_ms: Optional[int] = None,
        end_time_ms: Optional[int] = None,
        limit: Optional[int] = None,
    ) -> bytes:
        """
        Compressed/Aggregate Trades List.

        `GET /api/v3/aggTrades`

        Parameters
        ----------
        symbol : str
            The trading pair.
        from_id : int, optional
            The trade ID to fetch from. Default gets most recent trades.
        start_time_ms : int, optional
            The UNIX timestamp (milliseconds) to get aggregate trades from INCLUSIVE.
        end_time_ms: int, optional
            The UNIX timestamp (milliseconds) to get aggregate trades until INCLUSIVE.
        limit : int, optional
            The limit for the response. Default 500; max 1000.

        Returns
        -------
        bytes
            The raw response content.

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#compressed-aggregate-trades-list

        """
        payload: Dict[str, str] = {"symbol": symbol}
        if from_id is not None:
            payload["fromId"] = str(from_id)
        if start_time_ms is not None:
            payload["startTime"] = str(start_time_ms)
        if end_time_ms is not None:
            payload["endTime"] = str(end_time_ms)
        if limit is not None:
            payload["limit"] = str(limit)

        return await self.client.query(
            url_path=self.BASE_ENDPOINT + "aggTrades",
            payload=payload,
        )

    async def klines(
        self,
        symbol: str,
        interval: str,
        start_time_ms: Optional[int] = None,
        end_time_ms: Optional[int] = None,
        limit: Optional[int] = None,
    ) -> bytes:
        """
        Kline/Candlestick Data.

        `GET /api/v3/klines`

        Parameters
        ----------
        symbol : str
            The trading pair.
        interval : str
            The interval of kline, e.g 1m, 5m, 1h, 1d, etc.
        start_time_ms : int, optional
            The UNIX timestamp (milliseconds) to get aggregate trades from INCLUSIVE.
        end_time_ms: int, optional
            The UNIX timestamp (milliseconds) to get aggregate trades until INCLUSIVE.
        limit : int, optional
            The limit for the response. Default 500; max 1000.

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#kline-candlestick-data

        """
        payload: Dict[str, str] = {"symbol": symbol, "internal": interval}
        if start_time_ms is not None:
            payload["startTime"] = str(start_time_ms)
        if end_time_ms is not None:
            payload["endTime"] = str(end_time_ms)
        if limit is not None:
            payload["limit"] = str(limit)

        return await self.client.query(
            url_path=self.BASE_ENDPOINT + "klines",
            payload=payload,
        )

    async def avg_price(self, symbol: str) -> bytes:
        """
        Get the current average price for the given symbol.

        `GET /api/v3/avgPrice`

        Parameters
        ----------
        symbol : str
            The trading pair.

        Returns
        -------
        bytes
            The raw response content.

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#current-average-price

        """
        payload: Dict[str, str] = {"symbol": symbol}

        return await self.client.query(
            url_path=self.BASE_ENDPOINT + "avgPrice",
            payload=payload,
        )

    async def ticker_24hr(self, symbol: str = None) -> bytes:
        """
        24hr Ticker Price Change Statistics.

        `GET /api/v3/ticker/24hr`

        Parameters
        ----------
        symbol : str, optional
            The trading pair.

        Returns
        -------
        bytes
            The raw response content.

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#24hr-ticker-price-change-statistics

        """
        payload: Dict[str, str] = {}
        if symbol is not None:
            payload["symbol"] = symbol

        return await self.client.query(
            url_path=self.BASE_ENDPOINT + "ticker/24hr",
            payload=payload,
        )

    async def ticker_price(self, symbol: str = None) -> bytes:
        """
        Symbol Price Ticker.

        `GET /api/v3/ticker/price`

        Parameters
        ----------
        symbol : str, optional
            The trading pair.

        Returns
        -------
        bytes
            The raw response content.

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#symbol-price-ticker

        """
        payload: Dict[str, str] = {}
        if symbol is not None:
            payload["symbol"] = symbol

        return await self.client.query(
            url_path=self.BASE_ENDPOINT + "ticker/price",
            payload=payload,
        )

    async def book_ticker(self, symbol: str = None) -> bytes:
        """
        Symbol Order Book Ticker.

        `GET /api/v3/ticker/bookTicker`

        Parameters
        ----------
        symbol : str, optional
            The trading pair.

        Returns
        -------
        bytes
            The raw response content.

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#symbol-order-book-ticker

        """
        payload: Dict[str, str] = {}
        if symbol is not None:
            payload["symbol"] = symbol

        return await self.client.query(
            url_path=self.BASE_ENDPOINT + "ticker/bookTicker",
            payload=payload,
        )
