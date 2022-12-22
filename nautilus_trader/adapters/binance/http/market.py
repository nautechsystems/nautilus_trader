# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import Any, Optional

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.functions import format_symbol
from nautilus_trader.adapters.binance.common.schemas import BinanceTrade
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.core.correctness import PyCondition


class BinanceMarketHttpAPI:
    """
    Provides access to the Binance Market HTTP REST API.

    Parameters
    ----------
    client : BinanceHttpClient
        The Binance REST API client.
    account_type : BinanceAccountType
        The Binance account type, used to select the endpoint prefix

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        client: BinanceHttpClient,
        account_type: BinanceAccountType,
    ):
        PyCondition.not_none(client, "client")
        self.client = client
        self._decoder_trades = msgspec.json.Decoder(list[BinanceTrade])

        if account_type == BinanceAccountType.SPOT:
            self.base_endpoint = "/api/v3/"
        elif account_type == BinanceAccountType.MARGIN:
            self.base_endpoint = "/api/v3/"
        elif account_type == BinanceAccountType.FUTURES_USDT:
            self.base_endpoint = "/fapi/v1/"
        elif account_type == BinanceAccountType.FUTURES_COIN:
            self.base_endpoint = "/dapi/v1/"
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"invalid `BinanceAccountType`, was {account_type}",  # pragma: no cover
            )

    async def ping(self) -> dict[str, Any]:
        """
        Test the connectivity to the REST API.

        `GET /api/v3/ping`

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#test-connectivity
        https://binance-docs.github.io/apidocs/futures/en/#test-connectivity
        https://binance-docs.github.io/apidocs/delivery/en/#test-connectivity

        """
        raw: bytes = await self.client.query(url_path=self.base_endpoint + "ping")
        return msgspec.json.decode(raw)

    async def time(self) -> dict[str, Any]:
        """
        Test connectivity to the Rest API and get the current server time.

        Check Server Time.
        `GET /api/v3/time`

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#check-server-time
        https://binance-docs.github.io/apidocs/futures/en/#check-server-time
        https://binance-docs.github.io/apidocs/delivery/en/#check-server-time

        """
        raw: bytes = await self.client.query(url_path=self.base_endpoint + "time")
        return msgspec.json.decode(raw)

    async def depth(self, symbol: str, limit: Optional[int] = None) -> dict[str, Any]:
        """
        Get orderbook.

        `GET /api/v3/depth`

        Parameters
        ----------
        symbol : str
            The trading pair.
        limit : int, optional, default 100
            The limit for the response.
            SPOT/MARGIN (GET /api/v3/depth)
                Default 100; max 5000.
                Valid limits:[5, 10, 20, 50, 100, 500, 1000, 5000].
            FUTURES (GET /*api/v1/depth)
                Default 500; max 1000.
                Valid limits:[5, 10, 20, 50, 100, 500, 1000].

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#order-book
        https://binance-docs.github.io/apidocs/futures/en/#order-book
        https://binance-docs.github.io/apidocs/delivery/en/#order-book

        """
        payload: dict[str, str] = {"symbol": format_symbol(symbol)}
        if limit is not None:
            payload["limit"] = str(limit)

        raw: bytes = await self.client.query(
            url_path=self.base_endpoint + "depth",
            payload=payload,
        )

        return msgspec.json.decode(raw)

    async def trades(self, symbol: str, limit: Optional[int] = None) -> list[BinanceTrade]:
        """
        Get recent market trades.

        Recent Trades List.
        `GET /api/v3/trades`

        Parameters
        ----------
        symbol : str
            The trading pair.
        limit : int, optional
            The limit for the response. Default 500; max 1000.

        Returns
        -------
        list[BinanceTrade]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#recent-trades-list
        https://binance-docs.github.io/apidocs/futures/en/#recent-trades-list
        https://binance-docs.github.io/apidocs/delivery/en/#recent-trades-list

        """
        payload: dict[str, str] = {"symbol": format_symbol(symbol)}
        if limit is not None:
            payload["limit"] = str(limit)

        raw: bytes = await self.client.query(
            url_path=self.base_endpoint + "trades",
            payload=payload,
        )

        return self._decoder_trades.decode(raw)

    async def historical_trades(
        self,
        symbol: str,
        from_id: Optional[int] = None,
        limit: Optional[int] = None,
    ) -> dict[str, Any]:
        """
        Get older market trades.

        Old Trade Lookup.
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
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#old-trade-lookup
        https://binance-docs.github.io/apidocs/futures/en/#old-trades-lookup-market_data
        https://binance-docs.github.io/apidocs/delivery/en/#old-trades-lookup-market_data

        """
        payload: dict[str, str] = {"symbol": format_symbol(symbol)}
        if limit is not None:
            payload["limit"] = str(limit)
        if from_id is not None:
            payload["fromId"] = str(from_id)

        raw: bytes = await self.client.limit_request(
            http_method="GET",
            url_path=self.base_endpoint + "historicalTrades",
            payload=payload,
        )

        return msgspec.json.decode(raw)

    async def agg_trades(
        self,
        symbol: str,
        from_id: Optional[int] = None,
        start_time_ms: Optional[int] = None,
        end_time_ms: Optional[int] = None,
        limit: Optional[int] = None,
    ) -> dict[str, Any]:
        """
        Get recent aggregated market trades.

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
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#compressed-aggregate-trades-list
        https://binance-docs.github.io/apidocs/futures/en/#compressed-aggregate-trades-list
        https://binance-docs.github.io/apidocs/delivery/en/#compressed-aggregate-trades-list

        """
        payload: dict[str, str] = {"symbol": format_symbol(symbol)}
        if from_id is not None:
            payload["fromId"] = str(from_id)
        if start_time_ms is not None:
            payload["startTime"] = str(start_time_ms)
        if end_time_ms is not None:
            payload["endTime"] = str(end_time_ms)
        if limit is not None:
            payload["limit"] = str(limit)

        raw: bytes = await self.client.query(
            url_path=self.base_endpoint + "aggTrades",
            payload=payload,
        )

        return msgspec.json.decode(raw)

    async def klines(
        self,
        symbol: str,
        interval: str,
        start_time_ms: Optional[int] = None,
        end_time_ms: Optional[int] = None,
        limit: Optional[int] = None,
    ) -> list[list[Any]]:
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

        Returns
        -------
        list[list[Any]]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#kline-candlestick-data
        https://binance-docs.github.io/apidocs/futures/en/#kline-candlestick-data
        https://binance-docs.github.io/apidocs/delivery/en/#kline-candlestick-data

        """
        payload: dict[str, str] = {
            "symbol": format_symbol(symbol),
            "interval": interval,
        }
        if start_time_ms is not None:
            payload["startTime"] = str(start_time_ms)
        if end_time_ms is not None:
            payload["endTime"] = str(end_time_ms)
        if limit is not None:
            payload["limit"] = str(limit)

        raw: bytes = await self.client.query(
            url_path=self.base_endpoint + "klines",
            payload=payload,
        )

        return msgspec.json.decode(raw)

    async def ticker_24hr(self, symbol: Optional[str] = None) -> dict[str, Any]:
        """
        24hr Ticker Price Change Statistics.

        `GET /api/v3/ticker/24hr`

        Parameters
        ----------
        symbol : str, optional
            The trading pair.

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#24hr-ticker-price-change-statistics
        https://binance-docs.github.io/apidocs/futures/en/#24hr-ticker-price-change-statistics
        https://binance-docs.github.io/apidocs/delivery/en/#24hr-ticker-price-change-statistics

        """
        payload: dict[str, str] = {}
        if symbol is not None:
            payload["symbol"] = format_symbol(symbol)

        raw: bytes = await self.client.query(
            url_path=self.base_endpoint + "ticker/24hr",
            payload=payload,
        )

        return msgspec.json.decode(raw)

    async def ticker_price(self, symbol: Optional[str] = None) -> dict[str, Any]:
        """
        Symbol Price Ticker.

        `GET /api/v3/ticker/price`

        Parameters
        ----------
        symbol : str, optional
            The trading pair.

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#symbol-price-ticker
        https://binance-docs.github.io/apidocs/futures/en/#symbol-price-ticker
        https://binance-docs.github.io/apidocs/delivery/en/#symbol-price-ticker

        """
        payload: dict[str, str] = {}
        if symbol is not None:
            payload["symbol"] = format_symbol(symbol)

        raw: bytes = await self.client.query(
            url_path=self.base_endpoint + "ticker/price",
            payload=payload,
        )

        return msgspec.json.decode(raw)

    async def book_ticker(self, symbol: Optional[str] = None) -> dict[str, Any]:
        """
        Symbol Order Book Ticker.

        `GET /api/v3/ticker/bookTicker`

        Parameters
        ----------
        symbol : str, optional
            The trading pair.

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#symbol-order-book-ticker
        https://binance-docs.github.io/apidocs/futures/en/#symbol-order-book-ticker
        https://binance-docs.github.io/apidocs/delivery/en/#symbol-order-book-ticker

        """
        payload: dict[str, str] = {}
        if symbol is not None:
            payload["symbol"] = format_symbol(symbol).upper()

        raw: bytes = await self.client.query(
            url_path=self.base_endpoint + "ticker/bookTicker",
            payload=payload,
        )

        return msgspec.json.decode(raw)
