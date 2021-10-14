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

from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.parsing import convert_list_to_json_array
from nautilus_trader.core.correctness import PyCondition


class BinanceSpotHTTPAPI:
    """
    Provides access to the `Binance SPOT` REST HTTP API.
    """

    def __init__(self, client: BinanceHttpClient):
        """
        Initialize a new instance of the ``BinanceSpotHTTPAPI`` class.

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

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#test-connectivity

        """
        return await self.client.query(url_path="/api/v3/ping")

    async def time(self):
        """
        Check Server Time.

        Test connectivity to the Rest API and get the current server time.

        `GET /api/v3/time`

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#check-server-time

        """
        return await self.client.query(url_path="/api/v3/time")

    async def exchange_info(self, symbol: str = None, symbols: list = None):
        """
        Exchange Information.

        Current exchange trading rules and symbol information.
        Only either `symbol` or `symbols` should be passed.

        `GET /api/v3/exchangeinfo`

        Parameters
        ----------
        symbol : str, optional
            The trading pair
        symbols : list[str], optional
            The list of trading pairs.

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#exchange-information

        """
        if symbol and symbols:
            raise ValueError("symbol and symbols cannot be sent together.")
        PyCondition.type_or_none(symbols, list, "symbols")

        return await self.client.query(
            url_path="/api/v3/exchangeInfo",
            payload={"symbol": symbol, "symbols": convert_list_to_json_array(symbols)},
        )

    async def depth(self, symbol: str, **kwargs):
        """
        Get orderbook.

        `GET /api/v3/depth`

        Parameters
        ----------
        symbol : str
            The trading pair
        **kwargs : dict
            limit (int, optional): limit the results. Default 100; valid limits:[5, 10, 20, 50, 100, 500, 1000, 5000]

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#order-book

        """
        PyCondition.valid_string(symbol, "symbol")

        return await self.client.query(
            url_path="/api/v3/depth",
            payload={"symbol": symbol, **kwargs},
        )

    async def trades(self, symbol: str, **kwargs):
        """
        Recent Trades List.

        Get recent trades (up to last 500).

        `GET /api/v3/trades`

        Parameters
        ----------
        symbol : str
            The trading pair
        **kwargs : dict
            limit (int, optional): limit the results. Default 500; max 1000.

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#recent-trades-list

        """
        PyCondition.valid_string(symbol, "symbol")

        return await self.client.query(
            url_path="/api/v3/trades",
            payload={"symbol": symbol, **kwargs},
        )

    async def historical_trades(self, symbol: str, **kwargs):
        """
        Old Trade Lookup.

        Get older market trades.

        `GET /api/v3/historicalTrades`

        Parameters
        ----------
        symbol : str
            The trading pair
        **kwargs : dict
            limit (int, optional): limit the results. Default 500; max 1000.
            formId (int, optional): trade id to fetch from. Default gets most recent trades.

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#old-trade-lookup

        """
        PyCondition.valid_string(symbol, "symbol")

        return await self.client.limit_request(
            http_method="GET",
            url_path="/api/v3/historicalTrades",
            payload={"symbol": symbol, **kwargs},
        )

    async def agg_trades(self, symbol: str, **kwargs):
        """
        Compressed/Aggregate Trades List.

        `GET /api/v3/aggTrades`

        Parameters
        ----------
        symbol : str
            The trading pair
        **kwargs : dict
            limit (int, optional): limit the results. Default 500; max 1000.
            formId (int, optional): id to get aggregate trades from INCLUSIVE.
            startTime (int, optional): Timestamp in ms to get aggregate trades from INCLUSIVE.
            endTime (int, optional): Timestamp in ms to get aggregate trades until INCLUSIVE.

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#compressed-aggregate-trades-list

        """
        PyCondition.valid_string(symbol, "symbol")

        return await self.client.query(
            url_path="/api/v3/aggTrades",
            payload={"symbol": symbol, **kwargs},
        )

    async def klines(self, symbol: str, interval: str, **kwargs):
        """
        Kline/Candlestick Data.

        `GET /api/v3/klines`

        Parameters
        ----------
        symbol : str
            The trading pair.
        interval : str
            The interval of kline, e.g 1m, 5m, 1h, 1d, etc.
        **kwargs : dict
            limit (int, optional): limit the results. Default 500; max 1000.
            startTime (int, optional): Timestamp in ms to get aggregate trades from INCLUSIVE.
            endTime (int, optional): Timestamp in ms to get aggregate trades until INCLUSIVE.

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#kline-candlestick-data

        """
        PyCondition.valid_string(symbol, "symbol")
        PyCondition.valid_string(interval, "interval")

        return await self.client.query(
            url_path="/api/v3/klines",
            payload={"symbol": symbol, "interval": interval, **kwargs},
        )

    async def avg_price(self, symbol: str):
        """
        Get the current average price for the given symbol.

        `GET /api/v3/avgPrice`

        Parameters
        ----------
        symbol : str
            The trading pair

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#current-average-price

        """
        PyCondition.valid_string(symbol, "symbol")

        return await self.client.query(
            url_path="/api/v3/avgPrice",
            payload={"symbol": symbol},
        )

    async def ticker_24hr(self, symbol: str = None):
        """
        24hr Ticker Price Change Statistics.

        `GET /api/v3/ticker/24hr`

        Parameters
        ----------
        symbol : str
            The trading pair, optional

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#24hr-ticker-price-change-statistics

        """
        PyCondition.type_or_none(symbol, str, "symbol")

        return await self.client.query(
            url_path="/api/v3/ticker/24hr",
            payload={"symbol": symbol},
        )

    async def ticker_price(self, symbol: str = None):
        """
        Symbol Price Ticker.

        `GET /api/v3/ticker/price`

        Parameters
        ----------
        symbol : str
            The trading pair, optional

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#symbol-price-ticker

        """
        PyCondition.type_or_none(symbol, str, "symbol")

        return await self.client.query(
            url_path="/api/v3/ticker/price",
            payload={"symbol": symbol},
        )

    async def book_ticker(self, symbol: str = None):
        """
        Symbol Order Book Ticker.

        `GET /api/v3/ticker/bookTicker`

        Parameters
        ----------
        symbol : str
            The trading pair, optional

        References
        ----------
        https://binance-docs.github.io/apidocs/spot/en/#symbol-order-book-ticker

        """
        PyCondition.type_or_none(symbol, str, "symbol")

        return await self.client.query(
            url_path="/api/v3/ticker/bookTicker",
            payload={"symbol": symbol},
        )
