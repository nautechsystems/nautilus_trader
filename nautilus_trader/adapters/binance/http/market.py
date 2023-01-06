# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import Optional

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceKlineInterval
from nautilus_trader.adapters.binance.common.enums import BinanceMethodType
from nautilus_trader.adapters.binance.common.enums import BinanceSecurityType
from nautilus_trader.adapters.binance.common.schemas.market import BinanceAggTrades
from nautilus_trader.adapters.binance.common.schemas.market import BinanceDepth
from nautilus_trader.adapters.binance.common.schemas.market import BinanceKlines
from nautilus_trader.adapters.binance.common.schemas.market import BinanceTicker24hrs
from nautilus_trader.adapters.binance.common.schemas.market import BinanceTickerBooks
from nautilus_trader.adapters.binance.common.schemas.market import BinanceTickerPrices
from nautilus_trader.adapters.binance.common.schemas.market import BinanceTime
from nautilus_trader.adapters.binance.common.schemas.market import BinanceTrades
from nautilus_trader.adapters.binance.common.schemas.symbol import BinanceSymbol
from nautilus_trader.adapters.binance.common.schemas.symbol import BinanceSymbols
from nautilus_trader.adapters.binance.common.types import BinanceBar
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.endpoint import BinanceHttpEndpoint
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.orderbook.data import OrderBookSnapshot


class BinancePingHttp(BinanceHttpEndpoint):
    """
    Endpoint for testing connectivity to the REST API.

    `GET /api/v3/ping`
    `GET /fapi/v1/ping`
    `GET /dapi/v1/ping`

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#test-connectivity
    https://binance-docs.github.io/apidocs/futures/en/#test-connectivity
    https://binance-docs.github.io/apidocs/delivery/en/#test-connectivity

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            BinanceMethodType.GET: BinanceSecurityType.NONE,
        }
        url_path = base_endpoint + "ping"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self.get_resp_decoder = msgspec.json.Decoder()

    async def _get(self) -> dict:
        method_type = BinanceMethodType.GET
        raw = await self._method(method_type, None)
        return self.get_resp_decoder.decode(raw)


class BinanceTimeHttp(BinanceHttpEndpoint):
    """
    Endpoint for testing connectivity to the REST API and receiving current server time.

    `GET /api/v3/time`
    `GET /fapi/v1/time`
    `GET /dapi/v1/time`

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#check-server-time
    https://binance-docs.github.io/apidocs/futures/en/#check-server-time
    https://binance-docs.github.io/apidocs/delivery/en/#check-server-time

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            BinanceMethodType.GET: BinanceSecurityType.NONE,
        }
        url_path = base_endpoint + "time"
        super().__init__(client, methods, url_path)
        self.get_resp_decoder = msgspec.json.Decoder(BinanceTime)

    async def _get(self) -> BinanceTime:
        method_type = BinanceMethodType.GET
        raw = await self._method(method_type, None)
        return self.get_resp_decoder.decode(raw)

    async def request_server_time(self) -> int:
        """Request server time from Binance"""
        response = await self._get()
        return response.serverTime


class BinanceDepthHttp(BinanceHttpEndpoint):
    """
    Endpoint of orderbook depth

    `GET /api/v3/depth`
    `GET /fapi/v1/depth`
    `GET /dapi/v1/depth`

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#order-book
    https://binance-docs.github.io/apidocs/futures/en/#order-book
    https://binance-docs.github.io/apidocs/delivery/en/#order-book

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            BinanceMethodType.GET: BinanceSecurityType.NONE,
        }
        url_path = base_endpoint + "depth"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self.get_resp_decoder = msgspec.json.Decoder(BinanceDepth)

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        Orderbook depth GET endpoint parameters

        Parameters
        ----------
        symbol : str
            The trading pair.
        limit : int, optional, default 100
            The limit for the response.
            SPOT/MARGIN (GET /api/v3/depth)
                Default 100; max 5000.
            FUTURES (GET /*api/v1/depth)
                Default 500; max 1000.
                Valid limits:[5, 10, 20, 50, 100, 500, 1000].
        """

        symbol: BinanceSymbol
        limit: Optional[int] = None

    async def _get(self, parameters: GetParameters) -> BinanceDepth:
        method_type = BinanceMethodType.GET
        raw = await self._method(method_type, parameters)
        return self.get_resp_decoder.decode(raw)

    async def request_order_book_snapshot(
        self,
        instrument_id: InstrumentId,
        ts_init: int,
        parameters: GetParameters,
    ) -> OrderBookSnapshot:
        response = await self._get(parameters)
        return response._parse_to_order_book_snapshot(
            instrument_id=instrument_id,
            ts_init=ts_init,
        )


class BinanceTradesHttp(BinanceHttpEndpoint):
    """
    Endpoint of recent market trades.

    `GET /api/v3/trades`
    `GET /fapi/v1/trades`
    `GET /dapi/v1/trades`

    Parameters
    ----------
    symbol : str
        The trading pair.
    limit : int, optional
        The limit for the response. Default 500; max 1000.

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#recent-trades-list
    https://binance-docs.github.io/apidocs/futures/en/#recent-trades-list
    https://binance-docs.github.io/apidocs/delivery/en/#recent-trades-list

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            BinanceMethodType.GET: BinanceSecurityType.NONE,
        }
        url_path = base_endpoint + "trades"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self.get_resp_decoder = msgspec.json.Decoder(BinanceTrades)

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        GET parameters for recent trades

        Parameters
        ----------
        symbol : str
            The trading pair.
        limit : int, optional
            The limit for the response. Default 500; max 1000.
        """

        symbol: BinanceSymbol
        limit: Optional[int] = None

    async def _get(self, parameters: GetParameters) -> BinanceTrades:
        method_type = BinanceMethodType.GET
        raw = await self._method(method_type, parameters)
        return self.get_resp_decoder.decode(raw)

    async def request_trade_ticks(
        self,
        instrument_id: InstrumentId,
        parameters: GetParameters,
        ts_init: int,
    ) -> list[TradeTick]:
        """Request TradeTicks from Binance"""
        response = await self._get(parameters)
        return response._parse_to_trade_ticks(
            instrument_id=instrument_id,
            ts_init=ts_init,
        )


class BinanceHistoricalTradesHttp(BinanceHttpEndpoint):
    """
    Endpoint of older market historical trades

    `GET /api/v3/historicalTrades`
    `GET /fapi/v1/historicalTrades`
    `GET /dapi/v1/historicalTrades`

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#old-trade-lookup
    https://binance-docs.github.io/apidocs/futures/en/#old-trades-lookup-market_data
    https://binance-docs.github.io/apidocs/delivery/en/#old-trades-lookup-market_data

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            BinanceMethodType.GET: BinanceSecurityType.MARKET_DATA,
        }
        url_path = base_endpoint + "historicalTrades"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self.get_resp_decoder = msgspec.json.Decoder(BinanceTrades)

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        GET parameters for historical trades

        Parameters
        ----------
        symbol : str
            The trading pair.
        limit : int, optional
            The limit for the response. Default 500; max 1000.
        fromId : str, optional
            Trade id to fetch from. Default gets most recent trades
        """

        symbol: BinanceSymbol
        limit: Optional[int] = None
        fromId: Optional[str] = None

    async def _get(self, parameters: GetParameters) -> BinanceTrades:
        method_type = BinanceMethodType.GET
        raw = await self._method(method_type, parameters)
        return self.get_resp_decoder.decode(raw)

    async def request_historical_trade_ticks(
        self,
        instrument_id: InstrumentId,
        parameters: GetParameters,
        ts_init: int,
    ) -> list[TradeTick]:
        """Request historical TradeTicks from Binance"""
        response = await self._get(parameters)
        return response._parse_to_trade_ticks(
            instrument_id=instrument_id,
            ts_init=ts_init,
        )


class BinanceAggTradesHttp(BinanceHttpEndpoint):
    """
    Endpoint of compressed and aggregated market trades.
    Market trades that fill in 100ms with the same price and same taking side
    will have the quantity aggregated.

    `GET /api/v3/aggTrades`
    `GET /fapi/v1/aggTrades`
    `GET /dapi/v1/aggTrades`

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#compressed-aggregate-trades-list
    https://binance-docs.github.io/apidocs/futures/en/#compressed-aggregate-trades-list
    https://binance-docs.github.io/apidocs/delivery/en/#compressed-aggregate-trades-list
    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            BinanceMethodType.GET: BinanceSecurityType.NONE,
        }
        url_path = base_endpoint + "aggTrades"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self.get_resp_decoder = msgspec.json.Decoder(BinanceAggTrades)

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        GET parameters for aggregate trades

        Parameters
        ----------
        symbol : str
            The trading pair.
        limit : int, optional
            The limit for the response. Default 500; max 1000.
        fromId : str, optional
            Trade id to fetch from INCLUSIVE
        startTime : str, optional
            Timestamp in ms to get aggregate trades from INCLUSIVE
        endTime : str, optional
            Timestamp in ms to get aggregate trades until INCLUSIVE
        """

        symbol: BinanceSymbol
        limit: Optional[int] = None
        fromId: Optional[str] = None
        startTime: Optional[str] = None
        endTime: Optional[str] = None

    async def _get(self, parameters: GetParameters) -> BinanceAggTrades:
        method_type = BinanceMethodType.GET
        raw = await self._method(method_type, parameters)
        return self.get_resp_decoder.decode(raw)


class BinanceKlinesHttp(BinanceHttpEndpoint):
    """
    Endpoint of Kline/candlestick bars for a symbol.
    Klines are uniquely identified by their open time.

    `GET /api/v3/klines`
    `GET /fapi/v1/klines`
    `GET /dapi/v1/klines`

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#kline-candlestick-data
    https://binance-docs.github.io/apidocs/futures/en/#kline-candlestick-data
    https://binance-docs.github.io/apidocs/delivery/en/#kline-candlestick-data

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            BinanceMethodType.GET: BinanceSecurityType.NONE,
        }
        url_path = base_endpoint + "klines"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self.get_resp_decoder = msgspec.json.Decoder(BinanceKlines)

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        GET parameters for klines

        Parameters
        ----------
        symbol : str
            The trading pair.
        interval : str
            The interval of kline, e.g 1m, 5m, 1h, 1d, etc.
        limit : int, optional
            The limit for the response. Default 500; max 1000.
        startTime : str, optional
            Timestamp in ms to get klines from INCLUSIVE
        endTime : str, optional
            Timestamp in ms to get klines until INCLUSIVE
        """

        symbol: BinanceSymbol
        interval: BinanceKlineInterval
        limit: Optional[int] = None
        startTime: Optional[str] = None
        endTime: Optional[str] = None

    async def _get(self, parameters: GetParameters) -> BinanceKlines:
        method_type = BinanceMethodType.GET
        raw = await self._method(method_type, parameters)
        return self.get_resp_decoder.decode(raw)

    async def request_binance_bars(
        self,
        bar_type: BarType,
        parameters: GetParameters,
        ts_init: int,
    ) -> list[BinanceBar]:
        """Request Binance Bars"""
        response = await self._get(parameters)
        return response._parse_to_binance_bars(
            bar_type=bar_type,
            ts_init=ts_init,
        )


class BinanceTicker24hrHttp(BinanceHttpEndpoint):
    """
    Endpoint of 24 hour rolling window price change statistics

    `GET /api/v3/ticker/24hr`
    `GET /fapi/v1/ticker/24hr`
    `GET /dapi/v1/ticker/24hr`

    Warnings
    --------
    Care should be taken when accessing this endpoint with no symbol specified.
    The weight usage can be very large, which will likely cause rate limits to be hit.

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#24hr-ticker-price-change-statistics
    https://binance-docs.github.io/apidocs/futures/en/#24hr-ticker-price-change-statistics
    https://binance-docs.github.io/apidocs/delivery/en/#24hr-ticker-price-change-statistics
    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            BinanceMethodType.GET: BinanceSecurityType.NONE,
        }
        url_path = base_endpoint + "ticker/24hr"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self.get_resp_decoder = msgspec.json.Decoder(BinanceTicker24hrs)

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        GET parameters for 24hr ticker

        Parameters
        ----------
        symbol : str
            The trading pair. When given, endpoint will return a single BinanceTicker24hr
            When omitted, endpoint will return a list of BinanceTicker24hr for all trading pairs.
        symbols : str
            SPOT/MARGIN only!
            List of trading pairs. When given, endpoint will return a list of BinanceTicker24hr
        type : str
            SPOT/MARGIN only!
            Select between FULL and MINI 24hr ticker responses to save bandwidth.
        """

        symbol: Optional[BinanceSymbol] = None
        symbols: Optional[BinanceSymbols] = None  # SPOT/MARGIN only
        type: Optional[str] = None  # SPOT/MARIN only

    async def _get(self, parameters: GetParameters) -> BinanceTicker24hrs:
        method_type = BinanceMethodType.GET
        raw = await self._method(method_type, parameters)
        return self.get_resp_decoder.decode(raw)


class BinanceTickerPriceHttp(BinanceHttpEndpoint):
    """
    Endpoint of latest price for a symbol or symbols

    `GET /api/v3/ticker/price`
    `GET /fapi/v1/ticker/price`
    `GET /dapi/v1/ticker/price`

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#symbol-price-ticker
    https://binance-docs.github.io/apidocs/futures/en/#symbol-price-ticker
    https://binance-docs.github.io/apidocs/delivery/en/#symbol-price-ticker
    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            BinanceMethodType.GET: BinanceSecurityType.NONE,
        }
        url_path = base_endpoint + "ticker/price"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self.get_resp_decoder = msgspec.json.Decoder(BinanceTickerPrices)

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        GET parameters for price ticker

        Parameters
        ----------
        symbol : str
            The trading pair. When given, endpoint will return a single BinanceTickerPrice
            When omitted, endpoint will return a list of BinanceTickerPrice for all trading pairs.
        symbols : str
            SPOT/MARGIN only!
            List of trading pairs. When given, endpoint will return a list of BinanceTickerPrice
        """

        symbol: Optional[BinanceSymbol] = None
        symbols: Optional[BinanceSymbols] = None  # SPOT/MARGIN only

    async def _get(self, parameters: GetParameters) -> BinanceTickerPrices:
        method_type = BinanceMethodType.GET
        raw = await self._method(method_type, parameters)
        return self.get_resp_decoder.decode(raw)


class BinanceTickerBookHttp(BinanceHttpEndpoint):
    """
    Endpoint of best price/qty on the order book for a symbol or symbols

    `GET /api/v3/ticker/bookTicker`
    `GET /fapi/v1/ticker/bookTicker`
    `GET /dapi/v1/ticker/bookTicker`

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#symbol-order-book-ticker
    https://binance-docs.github.io/apidocs/futures/en/#symbol-order-book-ticker
    https://binance-docs.github.io/apidocs/delivery/en/#symbol-order-book-ticker
    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            BinanceMethodType.GET: BinanceSecurityType.NONE,
        }
        url_path = base_endpoint + "ticker/price"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self.get_resp_decoder = msgspec.json.Decoder(BinanceTickerBooks)

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        GET parameters for order book ticker

        Parameters
        ----------
        symbol : str
            The trading pair. When given, endpoint will return a single BinanceTickerBook
            When omitted, endpoint will return a list of BinanceTickerBook for all trading pairs.
        symbols : str
            SPOT/MARGIN only!
            List of trading pairs. When given, endpoint will return a list of BinanceTickerBook
        """

        symbol: Optional[BinanceSymbol] = None
        symbols: Optional[BinanceSymbols] = None  # SPOT/MARGIN only

    async def _get(self, parameters: GetParameters) -> BinanceTickerBooks:
        method_type = BinanceMethodType.GET
        raw = await self._method(method_type, parameters)
        return self.get_resp_decoder.decode(raw)


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

        if account_type.is_spot_or_margin:
            self.base_endpoint = "/api/v3/"
        elif account_type == BinanceAccountType.FUTURES_USDT:
            self.base_endpoint = "/fapi/v1/"
        elif account_type == BinanceAccountType.FUTURES_COIN:
            self.base_endpoint = "/dapi/v1/"
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"invalid `BinanceAccountType`, was {account_type}",  # pragma: no cover
            )

        # Create Endpoints
        self.endpoint_ping = BinancePingHttp(client, self.base_endpoint)
        self.endpoint_time = BinanceTimeHttp(client, self.base_endpoint)
        self.endpoint_depth = BinanceDepthHttp(client, self.base_endpoint)
        self.endpoint_trades = BinanceTradesHttp(client, self.base_endpoint)
        self.endpoint_historical_trades = BinanceHistoricalTradesHttp(client, self.base_endpoint)
        self.endpoint_agg_trades = BinanceAggTradesHttp(client, self.base_endpoint)
        self.endpoint_klines = BinanceKlinesHttp(client, self.base_endpoint)
        self.endpoint_ticker_24hr = BinanceTicker24hrHttp(client, self.base_endpoint)
        self.endpoint_ticker_price = BinanceTickerPriceHttp(client, self.base_endpoint)
        self.endpoint_ticker_book = BinanceTickerBookHttp(client, self.base_endpoint)
