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
from decimal import Decimal

from cpython.datetime cimport datetime
from cryptofeed.callback import TradeCallback
from cryptofeed.exchanges import Binance
from cryptofeed.defines import TRADES
import cryptofeed
import cryptofeed.feed
import ccxt

from nautilus_trader.adapters.binance.providers import BinanceInstrumentProvider
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


cdef class BinanceDataClient(LiveDataClient):
    """
    Provides a data client for the `Binance` exchange.
    """

    def __init__(
        self,
        client_rest not None: ccxt.Exchange,
        client_feed not None: cryptofeed.FeedHandler,
        LiveDataEngine engine not None,
        LiveClock clock not None,
        Logger logger not None,
    ):
        """
        Initialize a new instance of the `BinanceDataClient` class.

        Parameters
        ----------
        client_rest : ccxt.Exchange
            The Binance REST client.
        client_feed : cryptofeed.FeedHandler
            The Binance streaming feed client.
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
        Condition.true(client_rest.name == "Binance", "client.name == `Binance`")
        super().__init__(
            Venue("BINANCE"),
            engine,
            clock,
            logger,
            config={
                "unavailable_methods": [
                    self.request_quote_ticks.__name__,
                ],
            }
        )

        self._client_rest = client_rest
        self._client_feed = client_feed  # Reference to class
        self._instrument_provider = BinanceInstrumentProvider(
            client=client_rest,
            load_all=False,
        )
        self._is_connected = False

        # Subscriptions
        self._subscribed_instruments = set()
        self._subscribed_trade_ticks = {}  # type: dict[Symbol, cryptofeed.FeedHandler]
        self._subscribed_bars = {}             # type: dict[BarType, asyncio.Task]

        # Scheduled tasks
        self._update_instruments_task = None

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

        # Schedule subscribed instruments update
        delay = _SECONDS_IN_HOUR
        update = self._run_after_delay(delay, self._subscribed_instruments_update(delay))
        self._update_instruments_task = self._loop.create_task(update)

        self._is_connected = True
        self._log.info("Connected.")

    cpdef void disconnect(self) except *:
        """
        Disconnect the client.
        """
        self._log.info("Disconnecting...")

        self._loop.create_task(self._disconnect())

    async def _disconnect(self):
        # Cancel update instruments
        if self._update_instruments_task:
            self._update_instruments_task.cancel()

        stop_tasks = []
        for symbol, feed_handler in self._subscribed_trade_ticks.items():
            self._log.debug(f"Stopping <TradeTick> feed for {symbol.code}...")
            for feed, _ in feed_handler.feeds:
                stop_tasks.append(self._loop.create_task(feed.stop()))

        if stop_tasks:
            await asyncio.gather(*stop_tasks)

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
        self._instrument_provider = BinanceInstrumentProvider(
            client=self._client_rest,
            load_all=False,
        )

        self._subscribed_instruments = set()

        # Check all tasks have been popped and cancelled
        # assert len(self._subscribed_quote_ticks) == 0
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

        if symbol in self._subscribed_trade_ticks:
            return

        feed = cryptofeed.exchanges.Binance(
            pairs=[symbol.code.replace('/', '-')],
            channels=[TRADES],
            callbacks={TRADES: TradeCallback(self._on_trade_tick)},
        )

        feed_handler = self._client_feed()
        feed_handler.add_feed(feed)
        feed_handler.run(start_loop=False, install_signal_handlers=False)

        self._subscribed_trade_ticks[symbol] = feed_handler

        self._log.debug(f"Added {feed}.")

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

        if symbol not in self._subscribed_trade_ticks:
            return

        feed_handler = self._subscribed_trade_ticks.pop(symbol)

        for feed, _ in feed_handler.feeds:
            self._loop.create_task(feed.stop())

        self._log.debug(f"Removed {feed_handler}.")

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
            self._log.error(f"`request_bars` was called with a `price_type` argument "
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

    async def _run_after_delay(self, double delay, coro):
        await asyncio.sleep(delay)
        return await coro

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
            trades = await self._client_rest.fetch_trades(
                symbol=symbol.code,
                since=to_posix_ms(from_datetime) if from_datetime is not None else None,
                limit=limit,
            )
        except TypeError:
            # Temporary work around for testing
            trades = self._client_rest.fetch_trades
        except Exception as ex:
            self._log.error(str(ex))
            return

        if not trades:
            self._log.error("No data returned from fetch_trades.")
            return

        cdef list ticks = []  # type: list[TradeTick]
        cdef dict trade       # type: dict[str, object]
        for trade in trades:
            ticks.append(self._parse_trade_tick(instrument, trade))

        self._handle_trade_ticks(
            symbol,
            ticks,
            correlation_id,
        )

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
            data = await self._client_rest.fetch_ohlcv(
                symbol=bar_type.symbol.code,
                timeframe=timeframe,
                since=to_posix_ms(from_datetime) if from_datetime is not None else None,
                limit=limit,
            )
        except TypeError:
            # Temporary work around for testing
            data = self._client_rest.fetch_ohlcv
        except Exception as ex:
            self._log.error(str(ex))
            return

        if not data:
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

        self._handle_bars(
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

        self._handle_trade_tick(tick)

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
