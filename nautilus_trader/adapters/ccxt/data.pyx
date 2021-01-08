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
from nautilus_trader.model.bar cimport BarSpecification
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
        client not None,
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
                "name": f"CCXTDataClient-{client.name.upper()}",
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
        self._subscribed_instruments = set()   # type: set[Symbol]
        self._subscribed_quote_ticks = {}      # type: dict[Symbol, asyncio.Task]
        self._subscribed_trade_ticks = {}      # type: dict[Symbol, asyncio.Task]
        self._subscribed_bars = {}             # type: dict[BarType, asyncio.Task]

        # Schedule subscribed instruments update
        delay = _SECONDS_IN_HOUR
        update = self._run_after_delay(delay, self._subscribed_instruments_update(delay))
        self._update_instruments_task = self._loop.create_task(update)

        self._log.info(f"Initialized.")

    def __repr__(self) -> str:
        return f"{type(self).__name__}"

    async def _run_after_delay(self, double delay, coro):
        await asyncio.sleep(delay)
        return await coro

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

    @property
    def subscribed_trade_ticks(self):
        """
        The trade tick symbols subscribed to.

        Returns
        -------
        list[Symbol]

        """
        return sorted(list(self._subscribed_trade_ticks.keys()))

    @property
    def subscribed_bars(self):
        """
        The bar types subscribed to.

        Returns
        -------
        list[BarType]

        """
        return sorted(list(self._subscribed_bars.keys()))

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
        self._loop.create_task(self._disconnect())

    async def _disconnect(self):
        self._log.info("Disconnecting...")

        # Cancel update instruments
        if self._update_instruments_task:
            self._update_instruments_task.cancel()

        # Cancel residual tasks
        for task in self._subscribed_trade_ticks.values():
            if not task.cancelled():
                self._log.debug(f"Cancelling {task}...")
                task.cancel()

        # Ensure ccxt streams closed
        self._log.info("Closing exchange...")
        await self._client.close()

        self._is_connected = False

        self._log.info("Disconnected.")

    cpdef void reset(self) except *:
        """
        Reset the client.
        """
        if self._is_connected:
            self._log.error("Cannot reset a connected data client.")
            return

        self._log.info("Resetting...")

        # TODO: Reset client
        self._instrument_provider = CCXTInstrumentProvider(
            client=self._client,
            load_all=False,
        )

        self._subscribed_instruments = set()

        # Check all tasks have been popped and cancelled
        assert len(self._subscribed_quote_ticks) == 0
        assert len(self._subscribed_trade_ticks) == 0
        assert len(self._subscribed_bars) == 0

        # Schedule subscribed instruments update
        delay = _SECONDS_IN_HOUR
        update = self._run_after_delay(delay, self._subscribed_instruments_update(delay))
        self._update_instruments_task = self._loop.create_task(update)

        self._log.info("Reset.")

    cpdef void dispose(self) except *:
        """
        Dispose the client.
        """
        if self._is_connected:
            self._log.error("Cannot dispose a connected data client.")
            return

        self._log.info("Disposing...")

        # Nothing to dispose yet

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

        if not self._client.has["watchOrderBook"]:
            self._log.error("`subscribe_quote_ticks` was called "
                            "when not supported by the exchange.")
            return

        if symbol in self._subscribed_quote_ticks:
            # TODO: Only call if not already subscribed
            self._log.debug(f"Already subscribed {symbol.code} <TradeTick> data.")
            return

        task = self._loop.create_task(self._watch_quotes(symbol))
        self._subscribed_quote_ticks[symbol] = task

        self._log.info(f"Subscribed to {symbol.code} <QuoteTick> data.")

    cpdef void subscribe_trade_ticks(self, Symbol symbol) except *:
        """
        Subscribe to `TradeTick` data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to subscribe to.

        """
        Condition.not_none(symbol, "symbol")

        if not self._client.has["watchTrades"]:
            self._log.error("`subscribe_trade_ticks` was called "
                            "when not supported by the exchange.")
            return

        if symbol in self._subscribed_trade_ticks:
            # TODO: Only call if not already subscribed
            self._log.debug(f"Already subscribed {symbol.code} <TradeTick> data.")
            return

        task = self._loop.create_task(self._watch_trades(symbol))
        self._subscribed_trade_ticks[symbol] = task

        self._log.info(f"Subscribed to {symbol.code} <TradeTick> data.")

    cpdef void subscribe_bars(self, BarType bar_type) except *:
        """
        Subscribe to `Bar` data for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to subscribe to.

        """
        Condition.not_none(bar_type, "bar_type")

        if not self._client.has["watchOHLCV"]:
            self._log.error("`subscribe_bars` was called "
                            "when not supported by the exchange.")
            return

        if bar_type.spec.price_type != PriceType.LAST:
            self._log.warning(f"`request_bars` was called with a `price_type` argument "
                              f"of `PriceType.{PriceTypeParser.to_str(bar_type.spec.price_type)}` "
                              f"when not supported by the exchange (must be LAST).")
            return

        if bar_type in self._subscribed_bars:
            # TODO: Only call if not already subscribed
            self._log.debug(f"Already subscribed {bar_type} <Bar> data.")
            return

        task = self._loop.create_task(self._watch_ohlcv(bar_type))
        self._subscribed_bars[bar_type] = task

        self._log.info(f"Subscribed to {bar_type} <Bar> data.")

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

        if symbol not in self._subscribed_quote_ticks:
            # TODO: Only call if subscribed
            self._log.debug(f"Not subscribed to {symbol.code} <QuoteTick> data.")
            return

        task = self._subscribed_quote_ticks.pop(symbol)
        task.cancel()
        self._log.debug(f"Cancelled {task}.")
        self._log.info(f"Unsubscribed from {symbol.code} <QuoteTick> data.")

    cpdef void unsubscribe_trade_ticks(self, Symbol symbol) except *:
        """
        Unsubscribe from `TradeTick` data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to unsubscribe from.

        """
        Condition.not_none(symbol, "symbol")

        if symbol not in self._subscribed_trade_ticks:
            # TODO: Only call if subscribed
            self._log.debug(f"Not subscribed to {symbol.code} <TradeTick> data.")
            return

        task = self._subscribed_trade_ticks.pop(symbol)
        task.cancel()
        self._log.debug(f"Cancelled {task}.")
        self._log.info(f"Unsubscribed from {symbol.code} <TradeTick> data.")

    cpdef void unsubscribe_bars(self, BarType bar_type) except *:
        """
        Unsubscribe from `Bar` data for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to unsubscribe from.

        """
        Condition.not_none(bar_type, "bar_type")

        if bar_type not in self._subscribed_bars:
            # TODO: Only call if subscribed
            self._log.debug(f"Not subscribed to {bar_type} <Bar> data.")
            return

        task = self._subscribed_bars.pop(bar_type)
        task.cancel()
        self._log.debug(f"Cancelled {task}.")
        self._log.info(f"Unsubscribed from {bar_type} <Bar> data.")

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

        self._loop.create_task(self._request_instrument(symbol, correlation_id))

    cpdef void request_instruments(self, UUID correlation_id) except *:
        """
        Request all instruments.

        Parameters
        ----------
        correlation_id : UUID
            The correlation identifier for the request.

        """
        Condition.not_none(correlation_id, "correlation_id")

        self._loop.create_task(self._request_instruments(correlation_id))

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

        self._log.warning("`request_quote_ticks` was called when not supported "
                          "by the exchange.")

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

        self._loop.create_task(self._request_trade_ticks(
            symbol,
            from_datetime,
            to_datetime,
            limit,
            correlation_id,
        ))

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
            self._log.warning(f"`request_bars` was called with a `price_type` argument "
                              f"of `PriceType.{PriceTypeParser.to_str(bar_type.spec.price_type)}` "
                              f"when not supported by the exchange (must be LAST).")
            return

        if to_datetime is not None:
            self._log.warning(f"`request_bars` was called with a `to_datetime` "
                              f"argument of `{to_datetime}` when not supported by the exchange "
                              f"(will use `limit` of {limit}).")

        self._loop.create_task(self._request_bars(
            bar_type,
            from_datetime,
            to_datetime,
            limit,
            correlation_id,
        ))

# -- INTERNAL --------------------------------------------------------------------------------------

    async def _watch_quotes(self, Symbol symbol):
        cdef Instrument instrument = self._instrument_provider.get(symbol)
        if instrument is None:
            self._log.error(f"Cannot subscribe to quote ticks (no instrument for {symbol.code}).")
            return

        # Setup precisions
        cdef int price_precision = instrument.price_precision
        cdef int size_precision = instrument.size_precision

        cdef list best_bid
        cdef list best_ask
        cdef bint exiting = False  # Flag to stop loop
        try:
            while True:
                try:
                    order_book = await self._client.watch_order_book(symbol.code, limit=5)
                except TypeError:
                    # Temporary workaround for testing
                    order_book = self._client.watch_order_book
                    exiting = True

                best_bid = order_book["bids"][0]
                best_ask = order_book["asks"][0]
                timestamp = order_book["timestamp"]
                if timestamp is None:  # Compiled to fast C check
                    # First quote timestamp often None
                    timestamp = self._client.milliseconds()
                self._on_quote_tick(
                    symbol,
                    best_bid[0],
                    best_ask[0],
                    best_bid[1],
                    best_ask[1],
                    timestamp,
                    price_precision,
                    size_precision,
                )

                if exiting:
                    break
        except asyncio.CancelledError as ex:
            self._log.debug(f"Cancelled `_watch_ticker` for {symbol.code}.")
        except Exception as ex:
            self._log.exception(ex)
        finally:
            # Finally close stream
            await self._client.close()

    cdef inline void _on_quote_tick(
        self,
        Symbol symbol,
        double best_bid,
        double best_ask,
        double best_bid_size,
        double best_ask_size,
        long timestamp,
        int price_precision,
        int size_precision,
    ) except *:
        cdef QuoteTick tick = QuoteTick(
            symbol,
            Price(best_bid, price_precision),
            Price(best_ask, price_precision),
            Quantity(best_bid_size, size_precision),
            Quantity(best_ask_size, size_precision),
            from_posix_ms(timestamp),
        )

        self._handle_quote_tick(tick)

    async def _watch_trades(self, Symbol symbol):
        cdef Instrument instrument = self._instrument_provider.get(symbol)
        if instrument is None:
            self._log.error(f"Cannot subscribe to trade ticks (no instrument for {symbol.code}).")
            return

        cdef int price_precision = instrument.price_precision
        cdef int size_precision = instrument.size_precision

        cdef trades  # TODO: Type ArrayCache
        cdef dict trade
        cdef bint exiting = False  # Flag to stop loop
        try:
            while True:
                try:
                    trades = await self._client.watch_trades(symbol.code)
                except TypeError:
                    # Temporary workaround for testing
                    trades = self._client.watch_trades
                    exiting = True

                for trade in trades:
                    self._on_trade_tick(
                        symbol,
                        trade["price"],
                        trade["amount"],
                        trade["side"],
                        trade["takerOrMaker"],
                        trade["id"],
                        trade["timestamp"],
                        price_precision,
                        size_precision,
                    )

                if exiting:
                    break
        except asyncio.CancelledError as ex:
            self._log.debug(f"Cancelled `_watch_trades` for {symbol.code}.")
        except Exception as ex:
            self._log.exception(ex)
        finally:
            # Finally close stream
            await self._client.close()

    cdef inline void _on_trade_tick(
        self,
        Symbol symbol,
        double price,
        double amount,
        str order_side,
        str liquidity_side,
        str trade_match_id,
        long timestamp,
        int price_precision,
        int size_precision,
    ) except *:
        # Determine liquidity side
        cdef OrderSide side = OrderSide.BUY if order_side == "buy" else OrderSide.SELL
        if liquidity_side == "maker":
            side = OrderSide.BUY if order_side == OrderSide.SELL else OrderSide.BUY

        cdef TradeTick tick = TradeTick(
            symbol,
            Price(price, price_precision),
            Quantity(amount, size_precision),
            side,
            TradeMatchId(trade_match_id),
            from_posix_ms(timestamp),
        )

        self._handle_trade_tick(tick)

    async def _watch_ohlcv(self, BarType bar_type):
        cdef Instrument instrument = self._instrument_provider.get(bar_type.symbol)
        if instrument is None:
            self._log.error(f"Cannot subscribe to bars (no instrument for {bar_type.symbol}).")
            return

        # Build timeframe
        cdef str timeframe = self._make_timeframe(bar_type.spec)
        if timeframe is None:
            self._log.warning(f"Requesting bars with BarAggregation."
                              f"{BarAggregationParser.to_str(bar_type.spec.aggregation)} "
                              f"not currently supported in this version.")
            return

        # Setup symbol constant and precisions
        cdef Symbol symbol = bar_type.symbol
        cdef int price_precision = instrument.price_precision
        cdef int size_precision = instrument.size_precision

        cdef long last_timestamp = 0
        cdef long this_timestamp = 0
        cdef bars  # TODO: Type ArrayCache
        cdef bar   # TODO: Type ArrayCache
        cdef bint exiting = False  # Flag to stop loop
        try:
            while True:
                try:
                    bars = await self._client.watch_ohlcv(symbol.code, timeframe=timeframe, limit=1)
                except TypeError:
                    # Temporary workaround for testing
                    bars = self._client.watch_ohlvc
                    exiting = True

                bar = bars[0]  # Last closed bar
                this_timestamp = bar[0]
                if last_timestamp == 0:
                    # Initialize last timestamp
                    last_timestamp = this_timestamp
                    continue
                elif this_timestamp != last_timestamp:
                    last_timestamp = this_timestamp

                    # Create new bar
                    self._on_bar(
                        bar_type,
                        bar[1],
                        bar[2],
                        bar[3],
                        bar[4],
                        bar[5],
                        this_timestamp,
                        price_precision,
                        size_precision,
                    )

                if exiting:
                    break
        except asyncio.CancelledError as ex:
            self._log.debug(f"Cancelled `_watch_ohlcv` for {symbol.code}.")
        except Exception as ex:
            self._log.exception(ex)
        finally:
            # Finally close stream
            await self._client.close()

    cdef inline void _on_bar(
        self,
        BarType bar_type,
        double open_price,
        double high_price,
        double low_price,
        double close_price,
        double volume,
        long timestamp,
        int price_precision,
        int size_precision,
    ) except *:
        cdef Bar bar = Bar(
            Price(open_price, price_precision),
            Price(high_price, price_precision),
            Price(low_price, price_precision),
            Price(close_price, price_precision),
            Quantity(volume, size_precision),
            from_posix_ms(timestamp),
        )

        self._handle_bar(bar_type, bar)

    async def _request_instrument(self, Symbol symbol, UUID correlation_id):
        await self._instrument_provider.load_all_async()
        cdef Instrument instrument = self._instrument_provider.get(symbol)
        if instrument is not None:
            self._handle_instruments([instrument], correlation_id)
        else:
            self._log.error(f"Could not find instrument {symbol.code}.")

    async def _request_instruments(self, correlation_id):
        await self._instrument_provider.load_all_async()
        cdef list instruments = list(self._instrument_provider.get_all().values())
        self._handle_instruments(instruments, correlation_id)

        self._log.info(f"Updated {len(instruments)} instruments.")
        self.initialized = True

    async def _subscribed_instruments_update(self, delay):
        await self._instrument_provider.load_all_async()

        cdef Symbol symbol
        cdef Instrument instrument
        for symbol in self._subscribed_instruments:
            instrument = self._instrument_provider.get(symbol)
            if instrument is not None:
                self._handle_instrument(instrument)
            else:
                self._log.error(f"Could not find instrument {symbol.code}.")

        # Reschedule subscribed instruments update
        update = self._run_after_delay(delay, self._subscribed_instruments_update(delay))
        self._update_instruments_task = self._loop.create_task(update)

    async def _request_trade_ticks(
        self,
        Symbol symbol,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID correlation_id,
    ):
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
            trades = await self._client.fetch_trades(
                symbol=symbol.code,
                since=to_posix_ms(from_datetime) if from_datetime is not None else None,
                limit=limit,
            )
        except TypeError:
            # Temporary work around for testing
            trades = self._client.fetch_trades
        except Exception as ex:
            self._log.exception(ex)
            return

        if not trades:
            self._log.error("No data returned from fetch_trades.")
            return

        # Setup precisions
        cdef int price_precision = instrument.price_precision
        cdef int size_precision = instrument.size_precision

        cdef list ticks = []  # type: list[TradeTick]
        cdef dict trade       # type: dict[str, object]
        for trade in trades:
            ticks.append(self._parse_trade_tick(symbol, trade, price_precision, size_precision))

        self._handle_trade_ticks(symbol, ticks, correlation_id)

    async def _request_bars(
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

        if bar_type.spec.is_time_aggregated():
            await self._request_time_bars(
                instrument,
                bar_type,
                from_datetime,
                to_datetime,
                limit,
                correlation_id,
            )

    async def _request_time_bars(
        self,
        Instrument instrument,
        BarType bar_type,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID correlation_id,
    ):
        # Build timeframe
        cdef str timeframe = self._make_timeframe(bar_type.spec)
        if timeframe is None:
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
            data = await self._client.fetch_ohlcv(
                symbol=bar_type.symbol.code,
                timeframe=timeframe,
                since=to_posix_ms(from_datetime) if from_datetime is not None else None,
                limit=limit,
            )
        except TypeError:
            # Temporary work around for testing
            data = self._client.fetch_ohlcv
        except Exception as ex:
            self._log.exception(ex)
            return

        if not data:
            self._log.error(f"No data returned for {bar_type}.")
            return

        # Setup precisions
        cdef int price_precision = instrument.price_precision
        cdef int size_precision = instrument.size_precision

        # Set partial bar
        cdef Bar partial_bar = self._parse_bar(data[-1], price_precision, size_precision)

        # Delete last values
        del data[-1]

        cdef list bars = []  # type: list[Bar]
        cdef list values     # type: list[object]
        for values in data:
            bars.append(self._parse_bar(values, price_precision, size_precision))

        self._handle_bars(
            bar_type,
            bars,
            partial_bar,
            correlation_id,
        )

    cdef inline TradeTick _parse_trade_tick(
        self,
        Symbol symbol,
        dict trade,
        int price_precision,
        int size_precision,
    ):
        return TradeTick(
            symbol,
            Price(trade['price'], price_precision),
            Quantity(trade['amount'], size_precision),
            OrderSide.BUY if trade["side"] == "buy" else OrderSide.SELL,
            TradeMatchId(trade["id"]),
            from_posix_ms(trade["timestamp"]),
        )

    cdef inline Bar _parse_bar(
        self,
        list values,
        int price_precision,
        int size_precision,
    ):
        return Bar(
            Price(values[1], price_precision),
            Price(values[2], price_precision),
            Price(values[3], price_precision),
            Price(values[4], price_precision),
            Quantity(values[5], size_precision),
            from_posix_ms(values[0]),
        )

    cdef str _make_timeframe(self, BarSpecification bar_spec):
        # Build timeframe
        cdef str timeframe = str(bar_spec.step)

        if bar_spec.aggregation == BarAggregation.MINUTE:
            timeframe += 'm'
        elif bar_spec.aggregation == BarAggregation.HOUR:
            timeframe += 'h'
        elif bar_spec.aggregation == BarAggregation.DAY:
            timeframe += 'd'
        else:
            return None  # Invalid aggregation

        return timeframe
