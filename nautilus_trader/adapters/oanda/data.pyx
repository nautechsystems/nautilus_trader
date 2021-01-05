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
# -------------------------------------------------------------------------------------------------

import asyncio
from cpython.datetime cimport datetime
import threading

import pandas as pd
import oandapyV20
from oandapyV20.endpoints.instruments import InstrumentsCandles
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
from nautilus_trader.model.bar cimport BarData
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.tick cimport QuoteTick

cdef int _SECONDS_IN_HOUR = 60 * 60


cdef class OandaDataClient(LiveDataClient):
    """
    Provides a data client for the `Oanda` brokerage.
    """

    def __init__(
        self,
        client not None: oandapyV20.API,
        str account_id not None,
        LiveDataEngine engine not None,
        LiveClock clock not None,
        Logger logger not None,
    ):
        """
        Initialize a new instance of the `OandaDataClient` class.

        Parameters
        ----------
        client : oandapyV20.API
            The Oanda client.
        account_id : str
            The Oanda account identifier.
        engine : LiveDataEngine
            The live data engine for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.

        """
        super().__init__(
            Venue("OANDA"),
            engine,
            clock,
            logger,
            config={
                "unavailable_methods": [
                    self.subscribe_trade_ticks.__name__,
                    self.request_quote_ticks.__name__,
                    self.request_trade_ticks.__name__,
                ],
            }
        )

        self._is_connected = False
        self._client = client
        self._account_id = account_id
        self._instrument_provider = OandaInstrumentProvider(
            client=self._client,
            account_id=self._account_id,
            load_all=False,
        )

        # Subscriptions
        self._subscribed_instruments = set()
        self._subscribed_quote_ticks = {}  # type: dict[Symbol, (threading.Event, asyncio.Future)]

        self._update_instruments_handle: asyncio.Handle = None

    def __repr__(self) -> str:
        return f"{type(self).__name__}"

    @property
    def subscribed_instruments(self):
        """
        The instruments subscribed to.

        Returns
        -------
        list[Symbol]

        """
        return sorted(list(self._subscribed_instruments))

    @property
    def subscribed_quote_ticks(self):
        """
        The quote tick symbols subscribed to.

        Returns
        -------
        list[Symbol]

        """
        return sorted(list(self._subscribed_quote_ticks.keys()))

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

        for symbol in self._subscribed_quote_ticks.copy():
            self.subscribe_quote_ticks(symbol)

        # Schedule subscribed instruments update
        self._update_instruments_handle: asyncio.Handle = self._loop.call_later(
            delay=_SECONDS_IN_HOUR,  # Every hour
            callback=self._subscribed_instruments_update,
        )

        self._is_connected = True
        self._log.info("Connected.")

    cpdef void disconnect(self) except *:
        """
        Disconnect the client.
        """
        self._log.info("Disconnecting...")

        for symbol in self._subscribed_quote_ticks.copy():
            self.unsubscribe_quote_ticks(symbol)

        if self._update_instruments_handle is not None:
            self._update_instruments_handle.cancel()

        self._log.debug(f"{self._update_instruments_handle}")

        self._is_connected = False
        self._log.info("Disconnected.")

    cpdef void reset(self) except *:
        """
        Reset the client.
        """
        self._client = oandapyV20.API(access_token=self._client.access_token)
        self._instrument_provider = OandaInstrumentProvider(
            client=self._client,
            account_id=self._account_id,
            load_all=False,
        )

        self._subscribed_instruments = set()
        self._subscribed_quote_ticks = {}

    cpdef void dispose(self) except *:
        """
        Dispose the client.
        """
        if self._is_connected:
            self.disconnect()

        self._log.info("Disposed.")

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

        if symbol not in self._subscribed_quote_ticks:
            event = threading.Event()
            future = self._loop.run_in_executor(None, self._stream_prices, symbol, event)
            self._subscribed_quote_ticks[symbol] = (event, future)

            self._log.debug(f"Subscribed to quote ticks for {symbol}.")

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

        if symbol in self._subscribed_quote_ticks:
            event, future = self._subscribed_quote_ticks.pop(symbol)
            event.set()
            future.cancel()

            self._log.debug(f"Unsubscribed from quote ticks for {symbol}.")

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
            The specified from datetime for the data.
        to_datetime : datetime, optional
            The specified to datetime for the data. If None then will default
            to the current datetime.
        limit : int
            The limit for the number of returned ticks.
        correlation_id : UUID
            The correlation identifier for the request.

        """
        Condition.not_none(symbol, "symbol")
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
            The specified from datetime for the data.
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
            The specified from datetime for the data.
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
                            f"when not supported by the brokerage (must be BID, ASK or MID).")
            return

        if to_datetime is not None:
            self._log.warning(f"`request_bars` was called with a `to_datetime` "
                              f"argument of `{to_datetime}` when not supported by the brokerage "
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

    cpdef void _request_instrument(self, Symbol symbol, UUID correlation_id) except *:
        self._instrument_provider.load_all()
        cdef Instrument instrument = self._instrument_provider.get(symbol)
        if instrument is not None:
            self._loop.call_soon_threadsafe(self._handle_instruments_py, [instrument], correlation_id)
        else:
            self._log.error(f"Could not find instrument {symbol.code}.")

    cpdef void _request_instruments(self, UUID correlation_id) except *:
        self._instrument_provider.load_all()
        cdef list instruments = list(self._instrument_provider.get_all().values())
        self._loop.call_soon_threadsafe(self._handle_instruments_py, instruments, correlation_id)

        self._log.info(f"Updated {len(instruments)} instruments.")
        self.initialized = True

    cpdef void _subscribed_instruments_update(self) except *:
        self._loop.run_in_executor(None, self._subscribed_instruments_load_and_send)

    cpdef void _subscribed_instruments_load_and_send(self) except *:
        self._instrument_provider.load_all()

        cdef Symbol symbol
        cdef Instrument instrument
        for symbol in self._subscribed_instruments:
            instrument = self._instrument_provider.get(symbol)
            if instrument is not None:
                self._loop.call_soon_threadsafe(self._handle_instrument_py, instrument)
            else:
                self._log.error(f"Could not find instrument {symbol.code}.")

        # Reschedule subscribed instruments update in one hour
        self._loop.call_later(_SECONDS_IN_HOUR, self._subscribed_instruments_update)

    cpdef void _request_bars(
        self,
        BarType bar_type,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID correlation_id,
    ) except *:
        cdef Instrument instrument = self._instrument_provider.get(bar_type.symbol)
        if instrument is None:
            self._log.error(f"Cannot request bars (no instrument for {bar_type.symbol}).")
            return

        oanda_name = instrument.info["name"]

        if bar_type.spec.price_type == PriceType.BID:
            pricing = "B"
        elif bar_type.spec.price_type == PriceType.ASK:
            pricing = "A"
        else:
            pricing = "M"

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
            self._log.error(f"Requesting bars with BarAggregation."
                            f"{BarAggregationParser.to_str(bar_type.spec.aggregation)} "
                            f"not currently supported in this version.")
            return

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
            "dailyAlignment": 0,  # UTC
            "count": limit,
            "price": pricing,
            "granularity": granularity,
        }

        # Account for partial bar
        if limit > 0:
            params["count"] = limit + 1

        if from_datetime is not None:
            params["start"] = format_iso8601(from_datetime)

        if to_datetime is not None:
            params["end"] = format_iso8601(to_datetime)

        cdef dict res
        try:
            req = InstrumentsCandles(instrument=oanda_name, params=params)
            res = self._client.request(req)
        except Exception as ex:
            self._log.error(str(ex))
            return

        cdef list data = res.get("candles", [])
        if len(data) == 0:
            self._log.error(f"No data returned for {bar_type}.")
            return

        # Parse all bars except for the last bar
        cdef list bars = []  # type: list[Bar]
        cdef dict values     # type: dict[str, object]
        for values in data:
            if not values["complete"]:
                continue
            bars.append(self._parse_bar(instrument, values, bar_type.spec.price_type))

        # Set partial bar if last bar not complete
        cdef dict last_values = data[-1]
        cdef Bar partial_bar = None
        if not last_values["complete"]:
            partial_bar = self._parse_bar(instrument, last_values, bar_type.spec.price_type)

        self._loop.call_soon_threadsafe(
            self._handle_bars_py,
            bar_type,
            bars,
            partial_bar,
            correlation_id,
        )

    cpdef void _stream_prices(self, Symbol symbol, event: threading.Event) except *:
        cdef dict res
        cdef dict best_bid
        cdef dict best_ask
        cdef QuoteTick tick
        try:
            params = {
                "instruments": symbol.code.replace('/', '_', 1),
                "sessionId": f"{symbol.code}-001",
            }

            req = PricingStream(accountID=self._account_id, params=params)

            while True:
                for res in self._client.request(req):
                    if event.is_set():
                        raise asyncio.CancelledError("Price stream stopped")
                    if res["type"] != "PRICE":
                        # Heartbeat
                        continue
                    tick = self._parse_quote_tick(symbol, res)
                    self._handle_quote_tick_py(tick)
        except asyncio.CancelledError:
            pass  # Expected cancellation
        except Exception as ex:
            self._log.exception(ex)

    cdef inline QuoteTick _parse_quote_tick(self, Symbol symbol, dict values):
        return QuoteTick(
            symbol,
            Price(values["bids"][0]["price"]),
            Price(values["asks"][0]["price"]),
            Quantity(1),
            Quantity(1),
            pd.to_datetime(values["time"]),
        )

    cdef inline Bar _parse_bar(self, Instrument instrument, dict values, PriceType price_type):
        cdef dict prices
        if price_type == PriceType.BID:
            prices = values["bid"]
        elif price_type == PriceType.ASK:
            prices = values["ask"]
        else:
            prices = values["mid"]

        return Bar(
            Price(prices["o"], instrument.price_precision),
            Price(prices["h"], instrument.price_precision),
            Price(prices["l"], instrument.price_precision),
            Price(prices["c"], instrument.price_precision),
            Quantity(values["volume"], instrument.size_precision),
            pd.to_datetime(values["time"]),
        )

# -- PYTHON WRAPPERS -------------------------------------------------------------------------------

    cpdef void _handle_instrument_py(self, Instrument instrument) except *:
        self._engine.process(instrument)

    cpdef void _handle_quote_tick_py(self, QuoteTick tick) except *:
        self._engine.process(tick)

    cpdef void _handle_trade_tick_py(self, TradeTick tick) except *:
        self._engine.process(tick)

    cpdef void _handle_bar_py(self, BarType bar_type, Bar bar) except *:
        self._engine.process(BarData(bar_type, bar))

    cpdef void _handle_instruments_py(self, list instruments, UUID correlation_id) except *:
        self._handle_instruments(instruments, correlation_id)

    cpdef void _handle_quote_ticks_py(self, Symbol symbol, list ticks, UUID correlation_id) except *:
        self._handle_quote_ticks(symbol, ticks, correlation_id)

    cpdef void _handle_trade_ticks_py(self, Symbol symbol, list ticks, UUID correlation_id) except *:
        self._handle_trade_ticks(symbol, ticks, correlation_id)

    cpdef void _handle_bars_py(self, BarType bar_type, list bars, Bar partial, UUID correlation_id) except *:
        self._handle_bars(bar_type, bars, partial, correlation_id)
