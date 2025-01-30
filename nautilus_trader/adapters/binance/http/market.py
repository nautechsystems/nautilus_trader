# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import sys
import time

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceKlineInterval
from nautilus_trader.adapters.binance.common.enums import BinanceSecurityType
from nautilus_trader.adapters.binance.common.schemas.market import BinanceAggTrade
from nautilus_trader.adapters.binance.common.schemas.market import BinanceDepth
from nautilus_trader.adapters.binance.common.schemas.market import BinanceKline
from nautilus_trader.adapters.binance.common.schemas.market import BinanceTicker24hr
from nautilus_trader.adapters.binance.common.schemas.market import BinanceTickerBook
from nautilus_trader.adapters.binance.common.schemas.market import BinanceTickerPrice
from nautilus_trader.adapters.binance.common.schemas.market import BinanceTime
from nautilus_trader.adapters.binance.common.schemas.market import BinanceTrade
from nautilus_trader.adapters.binance.common.symbol import BinanceSymbol
from nautilus_trader.adapters.binance.common.symbol import BinanceSymbols
from nautilus_trader.adapters.binance.common.types import BinanceBar
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.endpoint import BinanceHttpEndpoint
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import nanos_to_millis
from nautilus_trader.core.nautilus_pyo3 import HttpMethod
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.identifiers import InstrumentId


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
            HttpMethod.GET: BinanceSecurityType.NONE,
        }
        url_path = base_endpoint + "ping"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self._get_resp_decoder = msgspec.json.Decoder()

    async def get(self) -> dict:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, None)
        return self._get_resp_decoder.decode(raw)


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
            HttpMethod.GET: BinanceSecurityType.NONE,
        }
        url_path = base_endpoint + "time"
        super().__init__(client, methods, url_path)
        self._get_resp_decoder = msgspec.json.Decoder(BinanceTime)

    async def get(self) -> BinanceTime:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, None)
        return self._get_resp_decoder.decode(raw)


class BinanceDepthHttp(BinanceHttpEndpoint):
    """
    Endpoint of orderbook depth.

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
            HttpMethod.GET: BinanceSecurityType.NONE,
        }
        url_path = base_endpoint + "depth"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self._get_resp_decoder = msgspec.json.Decoder(BinanceDepth)

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        Orderbook depth GET endpoint parameters.

        Parameters
        ----------
        symbol : BinanceSymbol
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
        limit: int | None = None

    async def get(self, params: GetParameters) -> BinanceDepth:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, params)
        return self._get_resp_decoder.decode(raw)


class BinanceTradesHttp(BinanceHttpEndpoint):
    """
    Endpoint of recent market trades.

    `GET /api/v3/trades`
    `GET /fapi/v1/trades`
    `GET /dapi/v1/trades`

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
            HttpMethod.GET: BinanceSecurityType.NONE,
        }
        url_path = base_endpoint + "trades"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self._get_resp_decoder = msgspec.json.Decoder(list[BinanceTrade])

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        GET parameters for recent trades.

        Parameters
        ----------
        symbol : BinanceSymbol
            The trading pair.
        limit : int, optional
            The limit for the response. Default 500; max 1000.

        """

        symbol: BinanceSymbol
        limit: int | None = None

    async def get(self, params: GetParameters) -> list[BinanceTrade]:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, params)
        return self._get_resp_decoder.decode(raw)


class BinanceHistoricalTradesHttp(BinanceHttpEndpoint):
    """
    Endpoint of older market historical trades.

    `GET /api/v3/historicalTrades`
    `GET /fapi/v1/historicalTrades`
    `GET /dapi/v1/historicalTrades`

    References
    ----------
    https://binance-docs.github.io/apidocs/spot/en/#old-trade-lookup-market_data
    https://binance-docs.github.io/apidocs/futures/en/#old-trades-lookup-market_data
    https://binance-docs.github.io/apidocs/delivery/en/#old-trades-lookup-market_data

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        base_endpoint: str,
    ):
        methods = {
            HttpMethod.GET: BinanceSecurityType.MARKET_DATA,
        }
        url_path = base_endpoint + "historicalTrades"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self._get_resp_decoder = msgspec.json.Decoder(list[BinanceTrade])

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        GET parameters for historical trades.

        Parameters
        ----------
        symbol : BinanceSymbol
            The trading pair.
        limit : int, optional
            The limit for the response. Default 500; max 1000.
        fromId : int, optional
            Trade ID to fetch from. Default gets most recent trades

        """

        symbol: BinanceSymbol
        limit: int | None = None
        fromId: int | None = None

    async def get(self, params: GetParameters) -> list[BinanceTrade]:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, params)
        return self._get_resp_decoder.decode(raw)


class BinanceAggTradesHttp(BinanceHttpEndpoint):
    """
    Endpoint of compressed and aggregated market trades. Market trades that fill in
    100ms with the same price and same taking side will have the quantity aggregated.

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
            HttpMethod.GET: BinanceSecurityType.NONE,
        }
        url_path = base_endpoint + "aggTrades"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self._get_resp_decoder = msgspec.json.Decoder(list[BinanceAggTrade])

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        GET parameters for aggregate trades.

        Parameters
        ----------
        symbol : BinanceSymbol
            The trading pair.
        limit : int, optional
            The limit for the response. Default 500; max 1000.
        fromId : int, optional
            Trade ID to fetch from INCLUSIVE.
        startTime : int, optional
            Timestamp in ms to get aggregate trades from INCLUSIVE.
        endTime : int, optional
            Timestamp in ms to get aggregate trades until INCLUSIVE.

        """

        symbol: BinanceSymbol
        limit: int | None = None
        fromId: int | None = None
        startTime: int | None = None
        endTime: int | None = None

    async def get(self, params: GetParameters) -> list[BinanceAggTrade]:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, params)
        return self._get_resp_decoder.decode(raw)


class BinanceKlinesHttp(BinanceHttpEndpoint):
    """
    Endpoint of Kline/candlestick bars for a symbol. Klines are uniquely identified by
    their open time.

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
            HttpMethod.GET: BinanceSecurityType.NONE,
        }
        url_path = base_endpoint + "klines"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self._get_resp_decoder = msgspec.json.Decoder(list[BinanceKline])

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        GET parameters for klines.

        Parameters
        ----------
        symbol : BinanceSymbol
            The trading pair.
        interval : str
            The interval of kline, e.g 1m, 5m, 1h, 1d, etc.
        limit : int, optional
            The limit for the response. Default 500; max 1000.
        startTime : int, optional
            Timestamp in ms to get klines from INCLUSIVE.
        endTime : int, optional
            Timestamp in ms to get klines until INCLUSIVE.

        """

        symbol: BinanceSymbol
        interval: BinanceKlineInterval
        limit: int | None = None
        startTime: int | None = None
        endTime: int | None = None

    async def get(self, params: GetParameters) -> list[BinanceKline]:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, params)
        return self._get_resp_decoder.decode(raw)


class BinanceTicker24hrHttp(BinanceHttpEndpoint):
    """
    Endpoint of 24-hour rolling window price change statistics.

    `GET /api/v3/ticker/24hr`
    `GET /fapi/v1/ticker/24hr`
    `GET /dapi/v1/ticker/24hr`

    Warnings
    --------
    Care should be taken when accessing this endpoint with no symbol specified.
    The weight usage can be very large, which may cause rate limits to be hit.

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
            HttpMethod.GET: BinanceSecurityType.NONE,
        }
        url_path = base_endpoint + "ticker/24hr"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self._get_obj_resp_decoder = msgspec.json.Decoder(BinanceTicker24hr)
        self._get_arr_resp_decoder = msgspec.json.Decoder(list[BinanceTicker24hr])

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        GET parameters for 24hr ticker.

        Parameters
        ----------
        symbol : BinanceSymbol
            The trading pair. When given, endpoint will return a single BinanceTicker24hr
            When omitted, endpoint will return a list of BinanceTicker24hr for all trading pairs.
        symbols : BinanceSymbols
            SPOT/MARGIN only!
            List of trading pairs. When given, endpoint will return a list of BinanceTicker24hr.
        type : str
            SPOT/MARGIN only!
            Select between FULL and MINI 24hr ticker responses to save bandwidth.

        """

        symbol: BinanceSymbol | None = None
        symbols: BinanceSymbols | None = None  # SPOT/MARGIN only
        type: str | None = None  # SPOT/MARIN only

    async def _get(self, params: GetParameters) -> list[BinanceTicker24hr]:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, params)
        if params.symbol is not None:
            return [self._get_obj_resp_decoder.decode(raw)]
        else:
            return self._get_arr_resp_decoder.decode(raw)


class BinanceTickerPriceHttp(BinanceHttpEndpoint):
    """
    Endpoint of latest price for a symbol or symbols.

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
            HttpMethod.GET: BinanceSecurityType.NONE,
        }
        url_path = base_endpoint + "ticker/price"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self._get_obj_resp_decoder = msgspec.json.Decoder(BinanceTickerPrice)
        self._get_arr_resp_decoder = msgspec.json.Decoder(list[BinanceTickerPrice])

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        GET parameters for price ticker.

        Parameters
        ----------
        symbol : BinanceSymbol
            The trading pair. When given, endpoint will return a single BinanceTickerPrice.
            When omitted, endpoint will return a list of BinanceTickerPrice for all trading pairs.
        symbols : str
            SPOT/MARGIN only!
            List of trading pairs. When given, endpoint will return a list of BinanceTickerPrice.

        """

        symbol: BinanceSymbol | None = None
        symbols: BinanceSymbols | None = None  # SPOT/MARGIN only

    async def _get(self, params: GetParameters) -> list[BinanceTickerPrice]:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, params)
        if params.symbol is not None:
            return [self._get_obj_resp_decoder.decode(raw)]
        else:
            return self._get_arr_resp_decoder.decode(raw)


class BinanceTickerBookHttp(BinanceHttpEndpoint):
    """
    Endpoint of best price/qty on the order book for a symbol or symbols.

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
            HttpMethod.GET: BinanceSecurityType.NONE,
        }
        url_path = base_endpoint + "ticker/price"
        super().__init__(
            client,
            methods,
            url_path,
        )
        self._get_arr_resp_decoder = msgspec.json.Decoder(list[BinanceTickerBook])
        self._get_obj_resp_decoder = msgspec.json.Decoder(BinanceTickerBook)

    class GetParameters(msgspec.Struct, omit_defaults=True, frozen=True):
        """
        GET parameters for order book ticker.

        Parameters
        ----------
        symbol : str
            The trading pair. When given, endpoint will return a single BinanceTickerBook
            When omitted, endpoint will return a list of BinanceTickerBook for all trading pairs.
        symbols : str
            SPOT/MARGIN only!
            List of trading pairs. When given, endpoint will return a list of BinanceTickerBook.

        """

        symbol: BinanceSymbol | None = None
        symbols: BinanceSymbols | None = None  # SPOT/MARGIN only

    async def _get(self, params: GetParameters) -> list[BinanceTickerBook]:
        method_type = HttpMethod.GET
        raw = await self._method(method_type, params)
        if params.symbol is not None:
            return [self._get_obj_resp_decoder.decode(raw)]
        else:
            return self._get_arr_resp_decoder.decode(raw)


class BinanceMarketHttpAPI:
    """
    Provides access to the Binance Market HTTP REST API.

    Parameters
    ----------
    client : BinanceHttpClient
        The Binance REST API client.
    account_type : BinanceAccountType
        The Binance account type, used to select the endpoint prefix.

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
        elif account_type == BinanceAccountType.USDT_FUTURE:
            self.base_endpoint = "/fapi/v1/"
        elif account_type == BinanceAccountType.COIN_FUTURE:
            self.base_endpoint = "/dapi/v1/"
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"invalid `BinanceAccountType`, was {account_type}",  # pragma: no cover
            )

        # Create Endpoints
        self._endpoint_ping = BinancePingHttp(client, self.base_endpoint)
        self._endpoint_time = BinanceTimeHttp(client, self.base_endpoint)
        self._endpoint_depth = BinanceDepthHttp(client, self.base_endpoint)
        self._endpoint_trades = BinanceTradesHttp(client, self.base_endpoint)
        self._endpoint_historical_trades = BinanceHistoricalTradesHttp(client, self.base_endpoint)
        self._endpoint_agg_trades = BinanceAggTradesHttp(client, self.base_endpoint)
        self._endpoint_klines = BinanceKlinesHttp(client, self.base_endpoint)
        self._endpoint_ticker_24hr = BinanceTicker24hrHttp(client, self.base_endpoint)
        self._endpoint_ticker_price = BinanceTickerPriceHttp(client, self.base_endpoint)
        self._endpoint_ticker_book = BinanceTickerBookHttp(client, self.base_endpoint)

    async def ping(self) -> dict:
        """
        Ping Binance REST API.
        """
        return await self._endpoint_ping.get()

    async def request_server_time(self) -> int:
        """
        Request server time from Binance.
        """
        response = await self._endpoint_time.get()
        return response.serverTime

    async def query_depth(
        self,
        symbol: str,
        limit: int | None = None,
    ) -> BinanceDepth:
        """
        Query order book depth for a symbol.
        """
        return await self._endpoint_depth.get(
            params=self._endpoint_depth.GetParameters(
                symbol=BinanceSymbol(symbol),
                limit=limit,
            ),
        )

    async def request_order_book_snapshot(
        self,
        instrument_id: InstrumentId,
        ts_init: int,
        limit: int | None = None,
    ) -> OrderBookDeltas:
        """
        Request snapshot of order book depth.
        """
        depth = await self.query_depth(instrument_id.symbol.value, limit)
        return depth.parse_to_order_book_snapshot(
            instrument_id=instrument_id,
            ts_init=ts_init,
        )

    async def query_trades(
        self,
        symbol: str,
        limit: int | None = None,
    ) -> list[BinanceTrade]:
        """
        Query trades for symbol.
        """
        return await self._endpoint_trades.get(
            params=self._endpoint_trades.GetParameters(
                symbol=BinanceSymbol(symbol),
                limit=limit,
            ),
        )

    async def request_trade_ticks(
        self,
        instrument_id: InstrumentId,
        ts_init: int,
        limit: int | None = None,
    ) -> list[TradeTick]:
        """
        Request TradeTicks from Binance.
        """
        trades = await self.query_trades(instrument_id.symbol.value, limit)
        return [
            trade.parse_to_trade_tick(
                instrument_id=instrument_id,
                ts_init=ts_init,
            )
            for trade in trades
        ]

    async def query_agg_trades(
        self,
        symbol: str,
        limit: int | None = None,
        start_time: int | None = None,
        end_time: int | None = None,
        from_id: int | None = None,
    ) -> list[BinanceAggTrade]:
        """
        Query aggregated trades for symbol.
        """
        return await self._endpoint_agg_trades.get(
            params=self._endpoint_agg_trades.GetParameters(
                symbol=BinanceSymbol(symbol),
                limit=limit,
                startTime=start_time,
                endTime=end_time,
                fromId=from_id,
            ),
        )

    async def request_agg_trade_ticks(
        self,
        instrument_id: InstrumentId,
        ts_init: int,
        limit: int | None = 1000,
        start_time: int | None = None,
        end_time: int | None = None,
        from_id: int | None = None,
    ) -> list[TradeTick]:
        """
        Request TradeTicks from Binance aggregated trades.

        If start_time and end_time are both specified, will request *all* TradeTicks in
        the interval, making multiple requests if necessary.

        """
        ticks: list[TradeTick] = []
        next_start_time = start_time

        if end_time is None:
            end_time = sys.maxsize

        if from_id is not None and (start_time or end_time) is not None:
            raise RuntimeError(
                "Cannot specify both fromId and startTime or endTime.",
            )

        # Only split into separate requests if both start_time and end_time are specified
        max_interval = (1000 * 60 * 60) - 1  # 1ms under an hour, as specified in Futures docs.
        last_id = 0
        interval_limited = False

        def _calculate_next_end_time(start_time: int, end_time: int) -> tuple[int, bool]:
            next_interval = start_time + max_interval
            interval_limited = next_interval < end_time
            next_end_time = next_interval if interval_limited is True else end_time
            return next_end_time, interval_limited

        if start_time is not None and end_time is not None:
            next_end_time, interval_limited = _calculate_next_end_time(start_time, end_time)
        else:
            next_end_time = end_time

        while True:
            response = await self.query_agg_trades(
                instrument_id.symbol.value,
                limit,
                start_time=next_start_time,
                end_time=next_end_time,
                from_id=from_id,
            )

            for trade in response:
                if not trade.a > last_id:
                    # Skip duplicate trades
                    continue
                ticks.append(
                    trade.parse_to_trade_tick(
                        instrument_id=instrument_id,
                        ts_init=ts_init,
                    ),
                )

            if limit and len(response) < limit and interval_limited is False:
                # end loop regardless when limit is not hit
                break
            if (
                start_time is None
                or end_time is None
                or next_end_time >= nanos_to_millis(time.time_ns())
            ):
                break
            else:
                last = response[-1]
                last_id = last.a
                next_start_time = last.T
                next_end_time, interval_limited = _calculate_next_end_time(
                    next_start_time,
                    end_time,
                )
                continue

        return ticks

    async def query_historical_trades(
        self,
        symbol: str,
        limit: int | None = None,
        from_id: int | None = None,
    ) -> list[BinanceTrade]:
        """
        Query historical trades for symbol.
        """
        return await self._endpoint_historical_trades.get(
            params=self._endpoint_historical_trades.GetParameters(
                symbol=BinanceSymbol(symbol),
                limit=limit,
                fromId=from_id,
            ),
        )

    async def request_historical_trade_ticks(
        self,
        instrument_id: InstrumentId,
        ts_init: int,
        limit: int | None = None,
        from_id: int | None = None,
    ) -> list[TradeTick]:
        """
        Request historical TradeTicks from Binance.
        """
        historical_trades = await self.query_historical_trades(
            symbol=instrument_id.symbol.value,
            limit=limit,
            from_id=from_id,
        )
        return [
            trade.parse_to_trade_tick(
                instrument_id=instrument_id,
                ts_init=ts_init,
            )
            for trade in historical_trades
        ]

    async def query_klines(
        self,
        symbol: str,
        interval: BinanceKlineInterval,
        limit: int | None = None,
        start_time: int | None = None,
        end_time: int | None = None,
    ) -> list[BinanceKline]:
        """
        Query klines for a symbol over an interval.
        """
        return await self._endpoint_klines.get(
            params=self._endpoint_klines.GetParameters(
                symbol=BinanceSymbol(symbol),
                interval=interval,
                limit=limit,
                startTime=start_time,
                endTime=end_time,
            ),
        )

    async def request_binance_bars(
        self,
        bar_type: BarType,
        ts_init: int,
        interval: BinanceKlineInterval,
        limit: int | None = None,
        start_time: int | None = None,
        end_time: int | None = None,
    ) -> list[BinanceBar]:
        """
        Request Binance Bars from Klines.
        """
        end_time_ms = int(end_time) if end_time is not None else sys.maxsize
        all_bars: list[BinanceBar] = []
        while True:
            klines = await self.query_klines(
                symbol=bar_type.instrument_id.symbol.value,
                interval=interval,
                limit=limit,
                start_time=start_time,
                end_time=end_time,
            )
            bars: list[BinanceBar] = [
                kline.parse_to_binance_bar(bar_type, ts_init) for kline in klines
            ]
            all_bars.extend(bars)

            # Update the start_time to fetch the next set of bars
            if klines:
                next_start_time = klines[-1].open_time + 1
            else:
                # Handle the case when klines is empty
                break

            # No more bars to fetch
            if (limit and len(klines) < limit) or next_start_time >= end_time_ms:
                break

            start_time = next_start_time

        return all_bars

    async def query_ticker_24hr(
        self,
        symbol: str | None = None,
        symbols: list[str] | None = None,
        response_type: str | None = None,
    ) -> list[BinanceTicker24hr]:
        """
        Query 24hr ticker for symbol or symbols.
        """
        if symbol is not None and symbols is not None:
            raise RuntimeError(
                "Cannot specify both symbol and symbols parameters.",
            )
        return await self._endpoint_ticker_24hr._get(
            params=self._endpoint_ticker_24hr.GetParameters(
                symbol=BinanceSymbol(symbol) if symbol else None,
                symbols=BinanceSymbols(symbols) if symbols else None,
                type=response_type,
            ),
        )

    async def query_ticker_price(
        self,
        symbol: str | None = None,
        symbols: list[str] | None = None,
    ) -> list[BinanceTickerPrice]:
        """
        Query price ticker for symbol or symbols.
        """
        if symbol is not None and symbols is not None:
            raise RuntimeError(
                "Cannot specify both symbol and symbols parameters.",
            )
        return await self._endpoint_ticker_price._get(
            params=self._endpoint_ticker_price.GetParameters(
                symbol=BinanceSymbol(symbol) if symbol else None,
                symbols=BinanceSymbols(symbols) if symbols else None,
            ),
        )

    async def query_ticker_book(
        self,
        symbol: str | None = None,
        symbols: list[str] | None = None,
    ) -> list[BinanceTickerBook]:
        """
        Query book ticker for symbol or symbols.
        """
        if symbol is not None and symbols is not None:
            raise RuntimeError(
                "Cannot specify both symbol and symbols parameters.",
            )
        return await self._endpoint_ticker_book._get(
            params=self._endpoint_ticker_book.GetParameters(
                symbol=BinanceSymbol(symbol) if symbol else None,
                symbols=BinanceSymbols(symbols) if symbols else None,
            ),
        )
