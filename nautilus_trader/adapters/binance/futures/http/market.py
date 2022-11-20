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
from nautilus_trader.adapters.binance.common.functions import convert_symbols_list_to_json_array
from nautilus_trader.adapters.binance.common.functions import format_symbol
from nautilus_trader.adapters.binance.common.schemas import BinanceTrade
from nautilus_trader.adapters.binance.futures.schemas.market import BinanceFuturesExchangeInfo
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.core.correctness import PyCondition


class BinanceFuturesMarketHttpAPI:
    """
    Provides access to the `Binance Market` HTTP REST API.

    Parameters
    ----------
    client : BinanceHttpClient
        The Binance REST API client.
    """

    def __init__(
        self,
        client: BinanceHttpClient,
        account_type: BinanceAccountType = BinanceAccountType.FUTURES_USDT,
    ):
        PyCondition.not_none(client, "client")

        self.client = client
        self.account_type = account_type

        if self.account_type == BinanceAccountType.FUTURES_USDT:
            self.BASE_ENDPOINT = "/fapi/v1/"
        elif self.account_type == BinanceAccountType.FUTURES_COIN:
            self.BASE_ENDPOINT = "/dapi/v1/"
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"invalid `BinanceAccountType`, was {account_type}",  # pragma: no cover
            )

        self._decoder_exchange_info = msgspec.json.Decoder(BinanceFuturesExchangeInfo)
        self._decoder_trades = msgspec.json.Decoder(list[BinanceTrade])

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

        """
        raw: bytes = await self.client.query(url_path=self.BASE_ENDPOINT + "ping")
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

        """
        raw: bytes = await self.client.query(url_path=self.BASE_ENDPOINT + "time")
        return msgspec.json.decode(raw)

    async def exchange_info(
        self,
        symbol: Optional[str] = None,
        symbols: Optional[list[str]] = None,
    ) -> BinanceFuturesExchangeInfo:
        """
        Get current exchange trading rules and symbol information.
        Only either `symbol` or `symbols` should be passed.

        Exchange Information.
        `GET /api/v3/exchangeinfo`

        Parameters
        ----------
        symbol : str, optional
            The trading pair.
        symbols : list[str], optional
            The list of trading pairs.

        Returns
        -------
        BinanceFuturesExchangeInfo

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#exchange-information

        """
        if symbol and symbols:
            raise ValueError("`symbol` and `symbols` cannot be sent together")

        payload: dict[str, str] = {}
        if symbol is not None:
            payload["symbol"] = format_symbol(symbol)
        if symbols is not None:
            payload["symbols"] = convert_symbols_list_to_json_array(symbols)

        raw: bytes = await self.client.query(
            url_path=self.BASE_ENDPOINT + "exchangeInfo",
            payload=payload,
        )

        return self._decoder_exchange_info.decode(raw)

    async def depth(self, symbol: str, limit: Optional[int] = None) -> dict[str, Any]:
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
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#order-book

        """
        payload: dict[str, str] = {"symbol": format_symbol(symbol)}
        if limit is not None:
            payload["limit"] = str(limit)

        raw: bytes = await self.client.query(
            url_path=self.BASE_ENDPOINT + "depth",
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

        """
        payload: dict[str, str] = {"symbol": format_symbol(symbol)}
        if limit is not None:
            payload["limit"] = str(limit)

        raw: bytes = await self.client.query(
            url_path=self.BASE_ENDPOINT + "trades",
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

        """
        payload: dict[str, str] = {"symbol": format_symbol(symbol)}
        if limit is not None:
            payload["limit"] = str(limit)
        if from_id is not None:
            payload["fromId"] = str(from_id)

        raw: bytes = await self.client.limit_request(
            http_method="GET",
            url_path=self.BASE_ENDPOINT + "historicalTrades",
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
            url_path=self.BASE_ENDPOINT + "aggTrades",
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
            url_path=self.BASE_ENDPOINT + "klines",
            payload=payload,
        )

        return msgspec.json.decode(raw)

    async def avg_price(self, symbol: str) -> dict[str, Any]:
        """
        Get the current average price for the given symbol.

        `GET /api/v3/avgPrice`

        Parameters
        ----------
        symbol : str
            The trading pair.

        Returns
        -------
        dict[str, Any]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#current-average-price

        """
        payload: dict[str, str] = {"symbol": format_symbol(symbol)}

        raw: bytes = await self.client.query(
            url_path=self.BASE_ENDPOINT + "avgPrice",
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

        """
        payload: dict[str, str] = {}
        if symbol is not None:
            payload["symbol"] = format_symbol(symbol)

        raw: bytes = await self.client.query(
            url_path=self.BASE_ENDPOINT + "ticker/24hr",
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

        """
        payload: dict[str, str] = {}
        if symbol is not None:
            payload["symbol"] = format_symbol(symbol)

        raw: bytes = await self.client.query(
            url_path=self.BASE_ENDPOINT + "ticker/price",
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

        """
        payload: dict[str, str] = {}
        if symbol is not None:
            payload["symbol"] = format_symbol(symbol).upper()

        raw: bytes = await self.client.query(
            url_path=self.BASE_ENDPOINT + "ticker/bookTicker",
            payload=payload,
        )

        return msgspec.json.decode(raw)
