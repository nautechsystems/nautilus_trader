# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from cpython.datetime cimport datetime
import os

import pandas as pd
import oandapyV20
import oandapyV20.endpoints.instruments as instruments_endpoint
from oandapyV20.endpoints.pricing import PricingStream

from nautilus_trader.adapters.oanda.providers import OandaInstrumentProvider
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.constants cimport *  # str constants only
from nautilus_trader.core.datetime cimport format_iso8601
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.live.data cimport LiveDataClient
from nautilus_trader.live.data cimport LiveDataEngine
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregationParser
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.c_enums.price_type cimport PriceTypeParser
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.tick cimport QuoteTick


cdef class OandaDataClient(LiveDataClient):
    """
    Provides a data client for the `Oanda` brokerage.
    """

    def __init__(
        self,
        dict credentials,
        LiveDataEngine engine,
        LiveClock clock,
        Logger logger,
    ):
        """
        Initialize a new instance of the `OandaDataClient` class.

        Parameters
        ----------
        credentials : dict[str, str]
            The API credentials for the client.
        engine : LiveDataEngine
            The live data engine for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.

        """
        Condition.not_none(credentials, "credentials")
        super().__init__(
            Venue("OANDA"),
            engine,
            clock,
            logger,
        )

        self._api_token = os.getenv(credentials.get("api_token", ""))
        self._account_id = os.getenv(credentials.get("account_id", ""))

        self._is_connected = False
        self._client = oandapyV20.API(access_token=self._api_token)
        self._instrument_provider = OandaInstrumentProvider(
            client=self._client,
            account_id=self._account_id,
            load_all=False,
        )

        self._subscribed_instruments = set()
        self._subscribed_quote_ticks = set()

        # Schedule subscribed instruments update in one hour
        self._loop.call_later(60 * 60, self._subscribed_instruments_update)

    def __repr__(self) -> str:
        return f"{type(self).__name__}"

    cpdef bint is_connected(self) except *:
        """
        Return a value indicating whether the client is connected.

        Returns
        -------
        bool
            True if connected, else False.

        """
        return self._is_connected

    cpdef void connect(self) except *:
        """
        Connect the client.
        """
        self._log.info("Connecting...")

        self._is_connected = True
        self._log.info("Connected.")

    cpdef void disconnect(self) except *:
        """
        Disconnect the client.
        """
        self._log.info("Disconnecting...")

        self._is_connected = False

        self._log.info("Disconnected.")

    cpdef void reset(self) except *:
        """
        Reset the client.
        """
        self._client = oandapyV20.API(access_token=self._api_token)
        self._instrument_provider = OandaInstrumentProvider(
            client=self._client,
            account_id=self._account_id,
            load_all=False,
        )

        self._subscribed_instruments = set()
        self._subscribed_quote_ticks = set()

    cpdef void dispose(self) except *:
        """
        Dispose the client.
        """
        self._subscribed_instruments = set()
        self._subscribed_quote_ticks = set()

# -- SUBSCRIPTIONS ---------------------------------------------------------------------------------

    cpdef void subscribe_instrument(self, Symbol symbol) except *:
        """
        Subscribe to `Instrument` data for the given symbol.

        Parameters
        ----------
        symbol : Instrument
            The instrument symbol to subscribe to.

        """
        Condition.not_none(symbol, "symbol")

        self._subscribed_instruments.add(symbol)

    cpdef void subscribe_quote_ticks(self, Symbol symbol) except *:
        """
        Subscribe to `QuoteTick` data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to subscribe to.

        """
        Condition.not_none(symbol, "symbol")

        self._subscribed_quote_ticks.add(symbol)
        self._loop.run_in_executor(None, self._stream_prices, symbol)

    cpdef void subscribe_trade_ticks(self, Symbol symbol) except *:
        """
        Subscribe to `TradeTick` data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to subscribe to.

        """
        Condition.not_none(symbol, "symbol")

        self._log.error(f"`subscribe_trade_ticks` was called when not supported by the brokerage.")

    cpdef void subscribe_bars(self, BarType bar_type) except *:
        """
        Subscribe to `Bar` data for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to subscribe to.

        """
        Condition.not_none(bar_type, "bar_type")

        self._log.error(f"`subscribe_bars` was called when not supported by the brokerage "
                        f"(use internal aggregation).")

    cpdef void unsubscribe_instrument(self, Symbol symbol) except *:
        """
        Unsubscribe from `Instrument` data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The instrument symbol to unsubscribe from.

        """
        Condition.not_none(symbol, "symbol")

        self._subscribed_instruments.discard(symbol)

    cpdef void unsubscribe_quote_ticks(self, Symbol symbol) except *:
        """
        Unsubscribe from `QuoteTick` data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to unsubscribe from.

        """
        Condition.not_none(symbol, "symbol")

        self._subscribed_quote_ticks.discard(symbol)

    cpdef void unsubscribe_trade_ticks(self, Symbol symbol) except *:
        """
        Unsubscribe from `TradeTick` data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to unsubscribe from.

        """
        Condition.not_none(symbol, "symbol")

        self._log.error(f"`unsubscribe_trade_ticks` was called when not supported by the brokerage.")

    cpdef void unsubscribe_bars(self, BarType bar_type) except *:
        """
        Unsubscribe from `Bar` data for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to unsubscribe from.

        """
        Condition.not_none(bar_type, "bar_type")

        self._log.error(f"`unsubscribe_bars` was called when not supported by the brokerage "
                        f"(use internal aggregation).")

# -- REQUESTS --------------------------------------------------------------------------------------

    cpdef void request_instrument(self, Symbol symbol, UUID correlation_id) except *:
        """
        Request the instrument for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the request.
        correlation_id : UUID
            The correlation identifier for the request.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(correlation_id, "correlation_id")

        self._loop.run_in_executor(None, self._request_instrument, symbol, correlation_id)

    cpdef void request_instruments(self, UUID correlation_id) except *:
        """
        Request all instruments.

        Parameters
        ----------
        correlation_id : UUID
            The correlation identifier for the request.

        """
        Condition.not_none(correlation_id, "correlation_id")

        self._loop.run_in_executor(None, self._request_instruments, correlation_id)

    cpdef void request_quote_ticks(
        self,
        Symbol symbol,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID correlation_id,
    ) except *:
        """
        Request historical quote ticks for the given parameters.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol for the request.
        from_datetime : datetime, optional
            The specified from datetime for the data
        to_datetime : datetime, optional
            The specified to datetime for the data. If None then will default
            to the current datetime.
        limit : int
            The limit for the number of returned ticks.
        correlation_id : UUID
            The correlation identifier for the request.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_negative_int(limit, "limit")
        Condition.not_none(correlation_id, "correlation_id")

        self._log.error(f"`request_quote_ticks` was called when not supported by the brokerage.")

    cpdef void request_trade_ticks(
        self,
        Symbol symbol,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID correlation_id,
    ) except *:
        """
        Request historical trade ticks for the given parameters.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol for the request.
        from_datetime : datetime, optional
            The specified from datetime for the data
        to_datetime : datetime, optional
            The specified to datetime for the data. If None then will default
            to the current datetime.
        limit : int
            The limit for the number of returned ticks.
        correlation_id : UUID
            The correlation identifier for the request.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_negative_int(limit, "limit")
        Condition.not_none(correlation_id, "correlation_id")

        self._log.error(f"`request_trade_ticks` was called when not supported by the brokerage.")

    cpdef void request_bars(
        self,
        BarType bar_type,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID correlation_id,
    ) except *:
        """
        Request historical bars for the given parameters.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the request.
        from_datetime : datetime, optional
            The specified from datetime for the data
        to_datetime : datetime, optional
            The specified to datetime for the data. If None then will default
            to the current datetime.
        limit : int
            The limit for the number of returned bars.
        correlation_id : UUID
            The correlation identifier for the request.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_negative_int(limit, "limit")
        Condition.not_none(correlation_id, "correlation_id")

        if bar_type.spec.price_type == PriceType.UNDEFINED or bar_type.spec.price_type == PriceType.LAST:
            self._log.error(f"`request_bars` was called with a `price_type` argument "
                            f"of `PriceType.{PriceTypeParser.to_str(bar_type.spec.price_type)}` "
                            f"when not supported by the exchange (must be BID, ASK or MID).")
            return

        if to_datetime is not None:
            self._log.warning(f"`request_bars` was called with a `to_datetime` "
                              f"argument of `{to_datetime}` when not supported by the exchange "
                              f"(will use `limit` of {limit}).")

        self._loop.run_in_executor(
            None,
            self._request_bars,
            bar_type,
            from_datetime,
            to_datetime,
            limit,
            correlation_id,
        )

# -- INTERNAL --------------------------------------------------------------------------------------

    cpdef Bar _parse_bar(self, Instrument instrument, dict values, PriceType price_type):
        cdef dict prices
        if price_type == PriceType.BID:
            # prices = values.get("bid")  # TODO: Always mid bars?
            prices = values.get("mid")
        elif price_type == PriceType.ASK:
            # prices = values.get("ask")  # TODO: Always mid bars?
            prices = values.get("mid")
        else:
            prices = values.get("mid")

        return Bar(
            open_price=Price(prices.get("o"), instrument.price_precision),
            high_price=Price(prices.get("h"), instrument.price_precision),
            low_price=Price(prices.get("l"), instrument.price_precision),
            close_price=Price(prices.get("c"), instrument.price_precision),
            volume=Quantity(values.get("volume"), instrument.size_precision),
            timestamp=pd.to_datetime(values.get("time")),
        )

    def _request_instrument(self, Symbol symbol, UUID correlation_id):
        self._instrument_provider.load_all()
        self._loop.call_soon_threadsafe(self._send_instrument_with_correlation, symbol, correlation_id)

    def _request_instruments(self, UUID correlation_id):
        self._instrument_provider.load_all()
        self._loop.call_soon_threadsafe(self._send_instruments, correlation_id)

    def _send_instrument_with_correlation(self, Symbol symbol, UUID correlation_id):
        cdef Instrument instrument = self._instrument_provider.get_all().get(symbol)
        if instrument is not None:
            self._handle_instruments([instrument], correlation_id)
        else:
            self._log.error(f"Could not find instrument {symbol.code}.")

    def _send_instrument(self, Symbol symbol):
        cdef Instrument instrument = self._instrument_provider.get_all().get(symbol)
        if instrument is not None:
            self._handle_instrument(instrument)
        else:
            self._log.error(f"Could not find instrument {symbol.code}.")

    def _send_instruments(self, UUID correlation_id):
        cdef list instruments = list(self._instrument_provider.get_all().values())
        self._handle_instruments(instruments, correlation_id)

        self._log.info(f"Updated {len(instruments)} instruments.")
        self.initialized = True

    def _send_quote_tick(self, QuoteTick tick):
        self._handle_quote_tick(tick)

    def _send_bar(self, BarType bar_type, Bar bar):
        self._handle_bar(bar_type, bar)

    def _send_bars(self, BarType bar_type, list bars, UUID correlation_id):
        self._handle_bars(bar_type, bars, correlation_id)

    def _subscribed_instruments_update(self):
        self._loop.run_in_executor(None, self._subscribed_instruments_load_and_send)

    def _subscribed_instruments_load_and_send(self):
        self._instrument_provider.load_all()

        cdef Symbol symbol
        for symbol in self._subscribed_instruments:
            self._loop.call_soon_threadsafe(self._send_instrument, symbol)

        # Reschedule subscribed instruments update in one hour
        self._loop.call_later(60 * 60, self._subscribed_instruments_update)

    def _stream_prices(
        self,
        Symbol symbol,
    ):
        params = {
            "instruments": symbol.code.replace('/', '_')
        }

        req = PricingStream(accountID=self._account_id, params=params)

        cdef dict res
        cdef dict best_bid
        cdef dict best_ask
        cdef QuoteTick tick
        while True:
            try:
                if symbol not in self._subscribed_quote_ticks:
                    break

                for res in self._client.request(req):
                    if res.get("type") != "PRICE":
                        # Heartbeat
                        continue

                    best_bid = res.get("bids")[0]
                    best_ask = res.get("asks")[0]

                    tick = QuoteTick(
                        symbol,
                        Price(best_bid["price"]),
                        Price(best_ask["price"]),
                        Quantity(best_bid["liquidity"]),
                        Quantity(best_ask["liquidity"]),
                        pd.to_datetime(res["time"])
                    )

                    self._loop.call_soon_threadsafe(self._send_quote_tick, tick)

            except Exception as ex:
                self._log.error(str(ex))
                break

    def _request_bars(
        self,
        BarType bar_type,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID correlation_id,
    ):
        cdef Instrument instrument = self._instrument_provider.get(bar_type.symbol)
        if instrument is None:
            self._log.error(f"Cannot request bars (no instrument for {bar_type.symbol}).")
            return

        oanda_name = instrument.info["name"]

        if bar_type.spec.price_type == PriceType.BID or bar_type.spec.price_type == PriceType.ASK:
            candle_format = "bidask"
        else:
            candle_format = "midpoint"

        cdef str granularity

        if bar_type.spec.aggregation == BarAggregation.SECOND:
            granularity = 'S'
        elif bar_type.spec.aggregation == BarAggregation.MINUTE:
            granularity = 'M'
        elif bar_type.spec.aggregation == BarAggregation.HOUR:
            granularity = 'H'
        elif bar_type.spec.aggregation == BarAggregation.DAY:
            granularity = 'D'
        else:
            self._log.error(f"Requesting bars with aggregation "
                            f"{BarAggregationParser.to_str(bar_type.spec.aggregation)} "
                            f"not currently supported in this version.")

        granularity += str(bar_type.spec.step)
        valid_granularities = [
            "S5",
            "S10",
            "S15",
            "S30",
            "M1",
            "M2",
            "M3",
            "M4",
            "M5",
            "M10",
            "M15",
            "M30",
            "H1",
            "H2",
            "H3",
            "H4",
            "H6",
            "H8",
            "H12",
            "D1",
        ]

        if granularity not in valid_granularities:
            self._log.error(f"Requesting bars with invalid granularity `{granularity}`, "
                            f"interpolation will be available in a future version, "
                            f"valid_granularities={valid_granularities}.")

        cdef dict params = {
            "dailyAlignment": 0,
            "count": limit,
            "candleFormat": candle_format,
            "granularity": granularity,  # TODO: Implement
        }

        if from_datetime is not None:
            params["start"] = format_iso8601(from_datetime)

        if to_datetime is not None:
            params["end"] = format_iso8601(to_datetime)

        cdef dict res
        try:
            req = instruments_endpoint.InstrumentsCandles(instrument=oanda_name, params=params)
            res = self._client.request(req)
        except Exception as ex:
            self._log.error(str(ex))
            return

        cdef list data = res.get("candles", [])
        if len(data) == 0:
            self._log.error(f"No data returned for {bar_type}.")
            return

        cdef list bars = []  # type: list[Bar]
        cdef dict values     # type: dict[str, object]
        for values in data:
            if not values["complete"]:
                continue
            bars.append(self._parse_bar(instrument, values, bar_type.spec.price_type))

        self._loop.call_soon_threadsafe(
            self._send_bars,
            bar_type,
            bars,
            correlation_id,
        )
