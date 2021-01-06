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

from decimal import Decimal

from cpython.datetime cimport datetime

try:
    import ccxtpro
except ImportError:
    raise ImportError("ccxtpro is not installed, installation instructions at https://ccxt.pro")

from nautilus_trader.adapters.ccxt.providers import CCXTInstrumentProvider
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.constants cimport *  # str constants only
from nautilus_trader.core.datetime cimport from_posix_ms
from nautilus_trader.core.datetime cimport to_posix_ms
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.live.data cimport LiveDataClient
from nautilus_trader.live.data cimport LiveDataEngine
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregationParser
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.c_enums.price_type cimport PriceTypeParser
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarData
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.identifiers cimport TradeMatchId
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick

cdef int _SECONDS_IN_HOUR = 60 * 60


cdef class CCXTDataClient(LiveDataClient):
    """
    Provides a data client for the `Binance` exchange.
    """

    def __init__(
        self,
        client not None: ccxtpro.Exchange,
        LiveDataEngine engine not None,
        LiveClock clock not None,
        Logger logger not None,
    ):
        """
        Initialize a new instance of the `CCXTDataClient` class.

        Parameters
        ----------
        client : ccxtpro.Exchange
            The unified CCXT client.
        engine : LiveDataEngine
            The live data engine for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.

        Raises
        ------
        ValueError
            If client_rest.name != 'Binance'.

        """
        super().__init__(
            Venue(client.name.upper()),
            engine,
            clock,
            logger,
            config={
                "unavailable_methods": [
                    self.request_quote_ticks.__name__,
                ],
            }
        )

        self._is_connected = False
        self._client = client
        self._instrument_provider = CCXTInstrumentProvider(
            client=client,
            load_all=False,
        )

        # Subscriptions
        self._subscribed_instruments = set()

        try:
            # Schedule subscribed instruments update in one hour
            self._loop.call_later(_SECONDS_IN_HOUR, self._subscribed_instruments_update)
        except RuntimeError as ex:
            self._log.error(str(ex))

        self._log.info(f"Initialized.")

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
    def subscribed_trade_ticks(self):
        """
        The quote tick symbols subscribed to.

        Returns
        -------
        list[Symbol]

        """
        return []

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
        # TODO: Reset client
        self._instrument_provider = CCXTInstrumentProvider(
            client=self._client,
            load_all=False,
        )

        self._subscribed_instruments = set()

        try:
            # Schedule subscribed instruments update in one hour
            self._loop.call_later(60 * 60, self._subscribed_instruments_update)
        except RuntimeError as ex:
            self._log.error(str(ex))

    cpdef void dispose(self) except *:
        """
        Dispose the client.
        """
        pass  # Nothing to dispose yet

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

        # TODO: Implement

    cpdef void subscribe_trade_ticks(self, Symbol symbol) except *:
        """
        Subscribe to `TradeTick` data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to subscribe to.

        """
        Condition.not_none(symbol, "symbol")

    cpdef void subscribe_bars(self, BarType bar_type) except *:
        """
        Subscribe to `Bar` data for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to subscribe to.

        """
        Condition.not_none(bar_type, "bar_type")

        # TODO: Implement

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

        # TODO: Implement

    cpdef void unsubscribe_trade_ticks(self, Symbol symbol) except *:
        """
        Unsubscribe from `TradeTick` data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to unsubscribe from.

        """
        Condition.not_none(symbol, "symbol")

        # TODO: Implement

    cpdef void unsubscribe_bars(self, BarType bar_type) except *:
        """
        Unsubscribe from `Bar` data for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to unsubscribe from.

        """
        Condition.not_none(bar_type, "bar_type")

        # TODO: Implement

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
        Condition.not_negative_int(limit, "limit")
        Condition.not_none(correlation_id, "correlation_id")

        self._log.error("`request_quote_ticks` was called when not supported "
                        "by the exchange, use trade ticks.")

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
        Condition.not_none(correlation_id, "correlation_id")

        if to_datetime is not None:
            self._log.warning(f"`request_trade_ticks` was called with a `to_datetime` "
                              f"argument of {to_datetime} when not supported by the exchange "
                              f"(will use `limit` of {limit}).")

        self._loop.run_in_executor(
            None,
            self._request_trade_ticks,
            symbol,
            from_datetime,
            to_datetime,
            limit,
            correlation_id,
        )

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
        Condition.not_none(correlation_id, "correlation_id")

        if bar_type.spec.price_type != PriceType.LAST:
            self._log.error(f"`request_bars` was called with a `price_type` argument "
                            f"of `PriceType.{PriceTypeParser.to_str(bar_type.spec.price_type)}` "
                            f"when not supported by the exchange (must be LAST).")
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

    cpdef void _request_trade_ticks(
        self,
        Symbol symbol,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID correlation_id,
    ) except *:
        cdef Instrument instrument = self._instrument_provider.get(symbol)
        if instrument is None:
            self._log.error(f"Cannot request trade ticks (no instrument for {symbol}).")
            return

        if limit == 0:
            limit = 1000

        if limit > 1000:
            self._log.warning(f"Requested trades with limit of {limit} when Binance limit=1000.")
            limit = 1000

        cdef list trades
        try:
            trades = self._client.fetch_trades(
                symbol=symbol.code,
                since=to_posix_ms(from_datetime) if from_datetime is not None else None,
                limit=limit,
            )
        except Exception as ex:
            self._log.error(str(ex))
            return

        if len(trades) == 0:
            self._log.error("No data returned from fetch_trades.")
            return

        cdef list ticks = []  # type: list[TradeTick]
        cdef dict trade       # type: dict[str, object]
        for trade in trades:
            ticks.append(self._parse_trade_tick(instrument, trade))

        self._loop.call_soon_threadsafe(
            self._handle_trade_ticks_py,
            symbol,
            ticks,
            correlation_id,
        )

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

        if bar_type.spec.is_time_aggregated():
            self._request_time_bars(
                instrument,
                bar_type,
                from_datetime,
                to_datetime,
                limit,
                correlation_id,
            )

    cpdef void _request_time_bars(
        self,
        Instrument instrument,
        BarType bar_type,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID correlation_id,
    ) except *:
        # Build timeframe
        cdef str timeframe = str(bar_type.spec.step)

        if bar_type.spec.aggregation == BarAggregation.MINUTE:
            timeframe += 'm'
        elif bar_type.spec.aggregation == BarAggregation.HOUR:
            timeframe += 'h'
        elif bar_type.spec.aggregation == BarAggregation.DAY:
            timeframe += 'd'
        else:
            self._log.error(f"Requesting bars with BarAggregation."
                            f"{BarAggregationParser.to_str(bar_type.spec.aggregation)} "
                            f"not currently supported in this version.")
            return

        if limit == 0:
            limit = 1000
        elif limit > 0:
            # Account for partial bar
            limit += 1

        if limit > 1001:
            self._log.warning(f"Requested bars {bar_type} with limit of {limit} when Binance limit=1000.")
            limit = 1000

        cdef list data
        try:
            data = self._client.fetch_ohlcv(
                symbol=bar_type.symbol.code,
                timeframe=timeframe,
                since=to_posix_ms(from_datetime) if from_datetime is not None else None,
                limit=limit,
            )
        except Exception as ex:
            self._log.error(str(ex))
            return

        if len(data) == 0:
            self._log.error(f"No data returned for {bar_type}.")
            return

        # Set partial bar
        cdef Bar partial_bar = self._parse_bar(instrument, data[-1])

        # Delete last values
        del data[-1]

        cdef list bars = []  # type: list[Bar]
        cdef list values     # type: list[object]
        for values in data:
            bars.append(self._parse_bar(instrument, values))

        self._loop.call_soon_threadsafe(
            self._handle_bars_py,
            bar_type,
            bars,
            partial_bar,
            correlation_id,
        )

    cpdef void _on_trade_tick(
        self,
        str feed,
        str pair,
        int order_id,
        double timestamp,
        str side,
        amount: Decimal,
        price: Decimal,
        double receipt_timestamp,
    ) except *:
        cdef Symbol symbol = Symbol(pair.replace('-', '/', 1), self.venue)
        cdef Instrument instrument = self._instrument_provider.get(symbol)
        cdef TradeTick tick = TradeTick(
            symbol,
            Price(price, instrument.price_precision),
            Quantity(amount, instrument.size_precision),
            OrderSide.BUY if side == "buy" else OrderSide.SELL,
            TradeMatchId(str(order_id)),
            from_posix_ms(<long>(timestamp * 1000))
        )

        self._handle_trade_tick_py(tick)

    cdef inline TradeTick _parse_trade_tick(self, Instrument instrument, dict trade):
        return TradeTick(
            instrument.symbol,
            Price(trade['price'], instrument.price_precision),
            Quantity(trade['amount'], instrument.size_precision),
            OrderSide.BUY if trade["side"] == "buy" else OrderSide.SELL,
            TradeMatchId(trade["id"]),
            from_posix_ms(trade["timestamp"]),
        )

    cdef inline Bar _parse_bar(self, Instrument instrument, list values):
        return Bar(
            Price(values[1], instrument.price_precision),
            Price(values[2], instrument.price_precision),
            Price(values[3], instrument.price_precision),
            Price(values[4], instrument.price_precision),
            Quantity(values[5], instrument.size_precision),
            from_posix_ms(values[0]),
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
