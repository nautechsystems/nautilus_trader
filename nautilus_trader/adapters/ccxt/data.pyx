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

from ccxt.base.errors import BaseError as CCXTError

from nautilus_trader.adapters.ccxt.providers import CCXTInstrumentProvider

from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport dt_to_unix_millis
from nautilus_trader.core.datetime cimport millis_to_nanos
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.live.data_client cimport LiveMarketDataClient
from nautilus_trader.live.data_engine cimport LiveDataEngine
from nautilus_trader.model.c_enums.aggressor_side cimport AggressorSide
from nautilus_trader.model.c_enums.aggressor_side cimport AggressorSideParser
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregationParser
from nautilus_trader.model.c_enums.book_level cimport BookLevel
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.c_enums.price_type cimport PriceTypeParser
from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.bar cimport BarSpecification
from nautilus_trader.model.data.bar cimport BarType
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orderbook.data cimport OrderBookSnapshot


cdef int _SECONDS_IN_HOUR = 60 * 60


cdef class CCXTDataClient(LiveMarketDataClient):
    """
    Provides a data client for the unified CCXT Pro API.
    """

    def __init__(
        self,
        client not None,
        LiveDataEngine engine not None,
        LiveClock clock not None,
        Logger logger not None,
    ):
        """
        Initialize a new instance of the ``CCXTDataClient`` class.

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
            client_id=ClientId(client.name.upper()),
            engine=engine,
            clock=clock,
            logger=logger,
            config={
                "name": f"CCXTDataClient-{client.name.upper()}",
                "unavailable_methods": [
                    self.request_quote_ticks.__name__,
                ],
            }
        )

        self._client = client
        self._instrument_provider = CCXTInstrumentProvider(
            client=client,
            load_all=False,
        )

        self.is_connected = False

        # Subscriptions
        self._subscribed_instruments = set()   # type: set[InstrumentId]
        self._subscribed_order_books = {}      # type: dict[InstrumentId, asyncio.Task]
        self._subscribed_quote_ticks = {}      # type: dict[InstrumentId, asyncio.Task]
        self._subscribed_trade_ticks = {}      # type: dict[InstrumentId, asyncio.Task]
        self._subscribed_bars = {}             # type: dict[BarType, asyncio.Task]

        # Scheduled tasks
        self._update_instruments_task = None

    @property
    def subscribed_instruments(self):
        """
        The instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        return sorted(list(self._subscribed_instruments))

    @property
    def subscribed_quote_ticks(self):
        """
        The quote tick instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        return sorted(list(self._subscribed_quote_ticks.keys()))

    @property
    def subscribed_trade_ticks(self):
        """
        The trade tick instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

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

    cpdef void connect(self) except *:
        """
        Connect the client.
        """
        self._log.info("Connecting...")

        # Schedule subscribed instruments update
        delay = _SECONDS_IN_HOUR
        update = self._run_after_delay(delay, self._subscribed_instruments_update(delay))
        self._update_instruments_task = self._loop.create_task(update)

        self._loop.create_task(self._connect())

    async def _connect(self):
        try:
            await self._load_instruments()
        except CCXTError as ex:
            self._log_ccxt_error(ex, self._connect.__name__)
            return

        for instrument in self._instrument_provider.get_all().values():
            self._handle_data(instrument)

        self.is_connected = True
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

        # Ensure ccxt closed
        self._log.info("Closing WebSocket(s)...")
        await self._client.close()

        self.is_connected = False
        self._log.info("Disconnected.")

    cpdef void reset(self) except *:
        """
        Reset the client.
        """
        if self.is_connected:
            self._log.error("Cannot reset a connected data client.")
            return

        self._log.info("Resetting...")

        self._instrument_provider = CCXTInstrumentProvider(
            client=self._client,
            load_all=False,
        )

        self._subscribed_instruments = set()

        # Check all tasks have been popped and canceled
        assert not self._subscribed_order_books
        assert not self._subscribed_quote_ticks
        assert not self._subscribed_trade_ticks
        assert not self._subscribed_bars

        self._log.info("Reset.")

    cpdef void dispose(self) except *:
        """
        Dispose the client.
        """
        if self.is_connected:
            self._log.error("Cannot dispose a connected data client.")
            return

        self._log.info("Disposing...")

        # Nothing to dispose yet

        self._log.info("Disposed.")

# -- SUBSCRIPTIONS ---------------------------------------------------------------------------------

    cpdef void subscribe_instrument(self, InstrumentId instrument_id) except *:
        """
        Subscribe to `Instrument` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to subscribe to.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._subscribed_instruments.add(instrument_id)

    cpdef void subscribe_order_book(
        self,
        InstrumentId instrument_id,
        BookLevel level,
        int depth=0,
        dict kwargs=None,
    ) except *:
        """
        Subscribe to `OrderBook` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to subscribe to.
        level : BookLevel
            The order book level (L1, L2, L3).
        depth : int, optional
            The maximum depth for the order book. A depth of 0 is maximum depth.
        kwargs : dict, optional
            The keyword arguments for exchange specific parameters.

        """
        if kwargs is None:
            kwargs = {}
        Condition.not_none(instrument_id, "instrument_id")

        if not self._client.has.get("watchOrderBook", False):
            raise RuntimeError(f"CCXT `watch_order_book` not available for {self._client.name}")

        if instrument_id in self._subscribed_order_books:
            self._log.warning(f"Already subscribed {instrument_id.symbol} <OrderBook> data.")
            return

        task = self._loop.create_task(self._watch_order_book(
            instrument_id=instrument_id,
            level=level,
            depth=depth,
            kwargs=kwargs,
        ))
        self._subscribed_order_books[instrument_id] = task

        self._log.info(f"Subscribed to {instrument_id.symbol} <OrderBook> data.")

    cpdef void subscribe_quote_ticks(self, InstrumentId instrument_id) except *:
        """
        Subscribe to `QuoteTick` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to subscribe to.

        """
        Condition.not_none(instrument_id, "instrument_id")

        if instrument_id in self._subscribed_quote_ticks:
            self._log.warning(f"Already subscribed {instrument_id.symbol} <TradeTick> data.")
            return

        task = self._loop.create_task(self._watch_quotes(instrument_id))
        self._subscribed_quote_ticks[instrument_id] = task

        self._log.info(f"Subscribed to {instrument_id.symbol} <QuoteTick> data.")

    cpdef void subscribe_trade_ticks(self, InstrumentId instrument_id) except *:
        """
        Subscribe to `TradeTick` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to subscribe to.

        """
        Condition.not_none(instrument_id, "instrument_id")

        if instrument_id in self._subscribed_trade_ticks:
            self._log.warning(f"Already subscribed {instrument_id.symbol} <TradeTick> data.")
            return

        task = self._loop.create_task(self._watch_trades(instrument_id))
        self._subscribed_trade_ticks[instrument_id] = task

        self._log.info(f"Subscribed to {instrument_id.symbol} <TradeTick> data.")

    cpdef void subscribe_bars(self, BarType bar_type) except *:
        """
        Subscribe to `Bar` data for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to subscribe to.

        """
        Condition.not_none(bar_type, "bar_type")

        if not self._client.has.get("watchOHLCV", False):
            raise RuntimeError(f"CCXT `watch_ohlcv` not available for {self._client.name}")

        if bar_type.spec.price_type != PriceType.LAST:
            self._log.warning(f"`subscribe_bars` was called with a `price_type` argument "
                              f"of `PriceType.{PriceTypeParser.to_str(bar_type.spec.price_type)}` "
                              f"when not supported by the exchange (must be LAST).")
            return

        if bar_type in self._subscribed_bars:
            self._log.warning(f"Already subscribed {bar_type} <Bar> data.")
            return

        task = self._loop.create_task(self._watch_ohlcv(bar_type))
        self._subscribed_bars[bar_type] = task

        self._log.info(f"Subscribed to {bar_type} <Bar> data.")

    cpdef void unsubscribe_instrument(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `Instrument` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to unsubscribe from.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._subscribed_instruments.discard(instrument_id)

    cpdef void unsubscribe_order_book(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `OrderBook` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to unsubscribe from.

        """
        Condition.not_none(instrument_id, "instrument_id")

        if instrument_id not in self._subscribed_order_books:
            self._log.debug(f"Not subscribed to {instrument_id.symbol} <OrderBook> data.")
            return

        task = self._subscribed_order_books.pop(instrument_id)
        task.cancel()
        self._log.debug(f"Cancelled {task}.")
        self._log.info(f"Unsubscribed from {instrument_id.symbol} <OrderBook> data.")

    cpdef void unsubscribe_quote_ticks(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `QuoteTick` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to unsubscribe from.

        """
        Condition.not_none(instrument_id, "instrument_id")

        if instrument_id not in self._subscribed_quote_ticks:
            self._log.debug(f"Not subscribed to {instrument_id.symbol} <QuoteTick> data.")
            return

        task = self._subscribed_quote_ticks.pop(instrument_id)
        task.cancel()
        self._log.debug(f"Cancelled {task}.")
        self._log.info(f"Unsubscribed from {instrument_id.symbol} <QuoteTick> data.")

    cpdef void unsubscribe_trade_ticks(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `TradeTick` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to unsubscribe from.

        """
        Condition.not_none(instrument_id, "instrument_id")

        if instrument_id not in self._subscribed_trade_ticks:
            self._log.debug(f"Not subscribed to {instrument_id.symbol} <TradeTick> data.")
            return

        task = self._subscribed_trade_ticks.pop(instrument_id)
        task.cancel()
        self._log.debug(f"Cancelled {task}.")
        self._log.info(f"Unsubscribed from {instrument_id.symbol} <TradeTick> data.")

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
            self._log.debug(f"Not subscribed to {bar_type} <Bar> data.")
            return

        task = self._subscribed_bars.pop(bar_type)
        task.cancel()
        self._log.debug(f"Cancelled {task}.")
        self._log.info(f"Unsubscribed from {bar_type} <Bar> data.")

# -- REQUESTS --------------------------------------------------------------------------------------

    cpdef void request_instrument(self, InstrumentId instrument_id, UUID correlation_id) except *:
        """
        Request the instrument for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the request.
        correlation_id : UUID
            The correlation ID for the request.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(correlation_id, "correlation_id")

        self._loop.create_task(self._request_instrument(instrument_id, correlation_id))

    cpdef void request_instruments(self, UUID correlation_id) except *:
        """
        Request all instruments.

        Parameters
        ----------
        correlation_id : UUID
            The correlation ID for the request.

        """
        Condition.not_none(correlation_id, "correlation_id")

        self._loop.create_task(self._request_instruments(correlation_id))

    cpdef void request_quote_ticks(
        self,
        InstrumentId instrument_id,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID correlation_id,
    ) except *:
        """
        Request historical quote ticks for the given parameters.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument ID for the request.
        from_datetime : datetime, optional
            The specified from datetime for the data.
        to_datetime : datetime, optional
            The specified to datetime for the data. If None then will default
            to the current datetime.
        limit : int
            The limit for the number of returned ticks.
        correlation_id : UUID
            The correlation ID for the request.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_negative_int(limit, "limit")
        Condition.not_none(correlation_id, "correlation_id")

        self._log.warning("`request_quote_ticks` was called when not supported "
                          "by the exchange.")

    cpdef void request_trade_ticks(
        self,
        InstrumentId instrument_id,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID correlation_id,
    ) except *:
        """
        Request historical trade ticks for the given parameters.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument ID for the request.
        from_datetime : datetime, optional
            The specified from datetime for the data.
        to_datetime : datetime, optional
            The specified to datetime for the data. If None then will default
            to the current datetime.
        limit : int
            The limit for the number of returned ticks.
        correlation_id : UUID
            The correlation ID for the request.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(correlation_id, "correlation_id")

        if to_datetime is not None:
            self._log.warning(f"`request_trade_ticks` was called with a `to_datetime` "
                              f"argument of {to_datetime} when not supported by the exchange "
                              f"(will use `limit` of {limit}).")

        self._loop.create_task(self._request_trade_ticks(
            instrument_id,
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
            The correlation ID for the request.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(correlation_id, "correlation_id")

        if not self._client.has.get("fetchOHLCV", False):
            raise RuntimeError(f"CCXT `fetch_ohlcv` not available for {self._client.name}")

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

    cdef void _log_ccxt_error(self, ex, str method_name) except *:
        self._log.warning(f"{type(ex).__name__}: {ex} in {method_name}")

    cdef int64_t _ccxt_to_timestamp_ns(self, int64_t millis) except *:
        return millis_to_nanos(millis)

# -- STREAMS ---------------------------------------------------------------------------------------

    # TODO: Possibly combine this with _watch_quotes
    async def _watch_order_book(self, InstrumentId instrument_id, BookLevel level, int depth, dict kwargs):
        cdef Instrument instrument = self._instrument_provider.find(instrument_id)
        if instrument is None:
            self._log.error(f"Cannot subscribe to order book (no instrument for {instrument_id.symbol}).")
            return

        cdef OrderBookSnapshot snapshot
        try:
            while True:
                try:
                    lob = await self._client.watch_order_book(
                        symbol=instrument_id.symbol.value,
                        limit=None if depth == 0 else depth,
                        params=kwargs,
                    )
                    timestamp_ms = lob["timestamp"]
                    if timestamp_ms is None:
                        # First quote timestamp often None
                        timestamp_ms = self._client.milliseconds()

                    bids = lob.get("bids")
                    asks = lob.get("asks")
                    if not bids:
                        continue
                    if not asks:
                        continue

                    # Currently inefficient while using CCXT. The order book
                    # is regenerated with a snapshot on every update.
                    snapshot = OrderBookSnapshot(
                        instrument_id=instrument.id,
                        level=level,
                        bids=list(bids),
                        asks=list(asks),
                        ts_event_ns=self._ccxt_to_timestamp_ns(millis=timestamp_ms),
                        ts_recv_ns=self._clock.timestamp_ns(),
                    )

                    self._handle_data(snapshot)
                except CCXTError as ex:
                    self._log_ccxt_error(ex, self._watch_order_book.__name__)
                    continue
        except asyncio.CancelledError as ex:
            self._log.debug(f"Cancelled `_watch_order_book` for {instrument_id.symbol}.")
        except Exception as ex:
            self._log.exception(ex)

    async def _watch_quotes(self, InstrumentId instrument_id):
        cdef Instrument instrument = self._instrument_provider.find(instrument_id)
        if instrument is None:
            self._log.error(f"Cannot subscribe to quote ticks (no instrument for {instrument_id.symbol}).")
            return

        # Setup precisions
        cdef int price_precision = instrument.price_precision
        cdef int size_precision = instrument.size_precision

        cdef list bids
        cdef list asks
        cdef bint generate_tick = False
        cdef list last_best_bid = None
        cdef list last_best_ask = None
        cdef list best_bid = None
        cdef list best_ask = None
        cdef bint exiting = False  # Flag to stop loop
        try:
            while True:
                try:
                    lob = await self._client.watch_order_book(symbol=instrument_id.symbol.value)
                except CCXTError as ex:
                    self._log_ccxt_error(ex, self._watch_quotes.__name__)
                    continue
                except TypeError:
                    # Temporary workaround for testing
                    lob = self._client.watch_order_book
                    exiting = True

                bids = <list>lob.get("bids")
                asks = <list>lob.get("asks")

                if bids:
                    best_bid = bids[0]
                else:
                    continue

                if asks:
                    best_ask = asks[0]
                else:
                    continue

                generate_tick = False
                # Cache last quotes if changed
                if last_best_bid is None or best_bid != last_best_bid:
                    last_best_bid = best_bid
                    generate_tick = True
                if last_best_ask is None or best_ask != last_best_ask:
                    last_best_ask = best_ask
                    generate_tick = True

                # Only generate quote tick on change to best bid or ask
                if not generate_tick:
                    continue

                timestamp = lob["timestamp"]
                if timestamp is None:  # Compiled to fast C check
                    # First quote timestamp often None
                    timestamp = self._client.milliseconds()

                self._on_quote_tick(
                    instrument_id,
                    best_bid[0],
                    best_ask[0],
                    best_bid[1],
                    best_ask[1],
                    timestamp,
                    self._clock.timestamp_ns(),
                    price_precision,
                    size_precision,
                )

                if exiting:
                    break
        except asyncio.CancelledError as ex:
            self._log.debug(f"Cancelled `_watch_ticker` for {instrument_id.symbol}.")
        except Exception as ex:
            self._log.exception(ex)

    cdef void _on_quote_tick(
        self,
        InstrumentId instrument_id,
        double best_bid,
        double best_ask,
        double best_bid_size,
        double best_ask_size,
        int64_t ts_event_ns,
        int64_t ts_recv_ns,
        int price_precision,
        int size_precision,
    ) except *:
        cdef QuoteTick tick = QuoteTick(
            instrument_id,
            Price(best_bid, price_precision),
            Price(best_ask, price_precision),
            Quantity(best_bid_size, size_precision),
            Quantity(best_ask_size, size_precision),
            ts_event_ns,
            ts_recv_ns,
        )

        self._handle_data(tick)

    async def _watch_trades(self, InstrumentId instrument_id):
        cdef Instrument instrument = self._instrument_provider.find(instrument_id)
        if instrument is None:
            self._log.error(f"Cannot subscribe to trade ticks (no instrument for {instrument_id.symbol}).")
            return

        cdef int price_precision = instrument.price_precision
        cdef int size_precision = instrument.size_precision

        cdef dict trade
        cdef bint exiting = False  # Flag to stop loop
        try:
            while True:
                try:
                    trades = await self._client.watch_trades(symbol=instrument_id.symbol.value)
                except CCXTError as ex:
                    self._log_ccxt_error(ex, self._watch_trades.__name__)
                    continue
                except TypeError:
                    # Temporary workaround for testing
                    trades = self._client.watch_trades
                    exiting = True

                trade = trades[0]  # Last trade only
                self._on_trade_tick(
                    instrument_id,
                    trade["price"],
                    trade["amount"],
                    trade["side"],
                    trade["id"],
                    self._ccxt_to_timestamp_ns(millis=trade["timestamp"]),
                    self._clock.timestamp_ns(),
                    price_precision,
                    size_precision,
                )

                if exiting:
                    break
        except asyncio.CancelledError as ex:
            self._log.debug(f"Cancelled `_watch_trades` for {instrument_id.symbol}.")
        except Exception as ex:
            self._log.exception(ex)

    cdef void _on_trade_tick(
        self,
        InstrumentId instrument_id,
        double price,
        double amount,
        str aggressor_side,
        str match_id,
        int64_t ts_event_ns,
        int64_t ts_recv_ns,
        int price_precision,
        int size_precision,
    ) except *:
        cdef TradeTick tick = TradeTick(
            instrument_id,
            Price(price, price_precision),
            Quantity(amount, size_precision),
            AggressorSideParser.from_str(aggressor_side.upper()),
            match_id,
            ts_event_ns,
            ts_recv_ns,
        )

        self._handle_data(tick)

    async def _watch_ohlcv(self, BarType bar_type):
        cdef Instrument instrument = self._instrument_provider.find(bar_type.instrument_id)
        if instrument is None:
            self._log.error(f"Cannot subscribe to bars (no instrument for {bar_type.instrument_id}).")
            return

        # Build timeframe
        cdef str timeframe = self._make_timeframe(bar_type.spec)
        if timeframe is None:
            self._log.warning(f"Requesting bars with BarAggregation."
                              f"{BarAggregationParser.to_str(bar_type.spec.aggregation)} "
                              f"not currently supported in this version.")
            return

        # Setup instrument_id constant and precisions
        cdef InstrumentId instrument_id = bar_type.instrument_id
        cdef int price_precision = instrument.price_precision
        cdef int size_precision = instrument.size_precision

        cdef int64_t last_timestamp = 0
        cdef int64_t this_timestamp = 0
        cdef bint exiting = False  # Flag to stop loop
        try:
            while True:
                try:
                    bars = await self._client.watch_ohlcv(
                        symbol=instrument_id.symbol.value,
                        timeframe=timeframe,
                        limit=1,
                    )
                except CCXTError as ex:
                    self._log_ccxt_error(ex, self._watch_ohlcv.__name__)
                    continue
                except TypeError:
                    # Temporary workaround for testing
                    bars = self._client.watch_ohlvc
                    exiting = True

                bar = bars[0]  # Last closed bar
                this_timestamp = self._ccxt_to_timestamp_ns(millis=bar[0])
                if last_timestamp == 0:
                    # Initialize last timestamp
                    last_timestamp = this_timestamp
                    continue
                elif this_timestamp != last_timestamp:
                    last_timestamp = this_timestamp

                    self._on_bar(
                        bar_type,
                        bar[1],
                        bar[2],
                        bar[3],
                        bar[4],
                        bar[5],
                        this_timestamp,
                        self._clock.timestamp_ns(),
                        price_precision,
                        size_precision,
                    )

                if exiting:
                    break
        except asyncio.CancelledError as ex:
            self._log.debug(f"Cancelled `_watch_ohlcv` for {instrument_id.symbol}.")
        except Exception as ex:
            self._log.exception(ex)

    cdef void _on_bar(
        self,
        BarType bar_type,
        double open_price,
        double high_price,
        double low_price,
        double close_price,
        double volume,
        int64_t ts_event_ns,
        int64_t ts_recv_ns,
        int price_precision,
        int size_precision,
    ) except *:
        cdef Bar bar = Bar(
            bar_type,
            Price(open_price, price_precision),
            Price(high_price, price_precision),
            Price(low_price, price_precision),
            Price(close_price, price_precision),
            Quantity(volume, size_precision),
            ts_event_ns,
            ts_recv_ns,
        )

        self._handle_data(bar)

    async def _run_after_delay(self, double delay, coro):
        await asyncio.sleep(delay)
        return await coro

    async def _load_instruments(self):
        await self._instrument_provider.load_all_async()
        self._log.info(f"Updated {self._instrument_provider.count} instruments.")

    async def _request_instrument(self, InstrumentId instrument_id, UUID correlation_id):
        await self._load_instruments()
        cdef Instrument instrument = self._instrument_provider.find(instrument_id)
        if instrument is not None:
            self._handle_instruments([instrument], correlation_id)
        else:
            self._log.error(f"Could not find instrument {instrument_id.symbol}.")

    async def _request_instruments(self, correlation_id):
        await self._load_instruments()
        cdef list instruments = list(self._instrument_provider.get_all().values())
        self._handle_instruments(instruments, correlation_id)

    async def _subscribed_instruments_update(self, delay):
        await self._instrument_provider.load_all_async()

        cdef InstrumentId instrument_id
        cdef Instrument instrument
        for instrument_id in self._subscribed_instruments:
            instrument = self._instrument_provider.find(instrument_id)
            if instrument is not None:
                self._handle_data(instrument)
            else:
                self._log.error(f"Could not find instrument {instrument_id.symbol}.")

        # Reschedule subscribed instruments update
        update = self._run_after_delay(delay, self._subscribed_instruments_update(delay))
        self._update_instruments_task = self._loop.create_task(update)

    async def _request_trade_ticks(
        self,
        InstrumentId instrument_id,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID correlation_id,
    ):
        cdef Instrument instrument = self._instrument_provider.find(instrument_id)
        if instrument is None:
            self._log.error(f"Cannot request trade ticks (no instrument for {instrument_id}).")
            return

        if limit == 0:
            limit = 1000
        elif limit > 1000:
            self._log.warning(f"Requested trades with limit of {limit} when limit=1000.")

        # Account for partial bar
        limit += 1
        limit = min(limit, 1000)

        cdef list trades
        try:
            trades = await self._client.fetch_trades(
                symbol=instrument_id.symbol.value,
                since=dt_to_unix_millis(from_datetime) if from_datetime is not None else None,
                limit=limit,
            )
        except CCXTError as ex:
            self._log_ccxt_error(ex, self._request_trade_ticks.__name__)
            return
        except TypeError:
            # Temporary work around for testing
            trades = self._client.fetch_trades

        if not trades:
            self._log.error("No data returned from fetch_trades.")
            return

        # Setup precisions
        cdef int price_precision = instrument.price_precision
        cdef int size_precision = instrument.size_precision

        cdef list ticks = []  # type: list[TradeTick]
        cdef dict trade       # type: dict[str, object]
        for trade in trades:
            ticks.append(self._parse_trade_tick(instrument_id, trade, price_precision, size_precision))

        self._handle_trade_ticks(instrument_id, ticks, correlation_id)

    async def _request_bars(
        self,
        BarType bar_type,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID correlation_id,
    ):
        cdef Instrument instrument = self._instrument_provider.find(bar_type.instrument_id)
        if instrument is None:
            self._log.error(f"Cannot request bars (no instrument for {bar_type.instrument_id}).")
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
        elif limit > 1000:
            self._log.warning(f"Requested bars {bar_type} with limit of {limit} when Binance limit=1000.")

        # Account for partial bar
        limit += 1
        limit = min(limit, 1000)

        cdef list data
        try:
            data = await self._client.fetch_ohlcv(
                symbol=bar_type.instrument_id.symbol.value,
                timeframe=timeframe,
                since=dt_to_unix_millis(from_datetime) if from_datetime is not None else None,
                limit=limit,
            )
        except TypeError:
            # Temporary work around for testing
            data = self._client.fetch_ohlcv
        except CCXTError as ex:
            self._log_ccxt_error(ex, self._request_time_bars.__name__)
            return

        if not data:
            self._log.error(f"No data returned for {bar_type}.")
            return

        # Setup precisions
        cdef int price_precision = instrument.price_precision
        cdef int size_precision = instrument.size_precision

        # Set partial bar
        cdef Bar partial_bar = self._parse_bar(
            bar_type,
            data[-1],
            price_precision,
            size_precision,
        )

        # Delete last values
        del data[-1]

        cdef list bars = []  # type: list[Bar]
        cdef list values     # type: list[object]
        for values in data:
            bars.append(self._parse_bar(
                bar_type,
                values,
                price_precision,
                size_precision,
            ))

        self._handle_bars(
            bar_type,
            bars,
            partial_bar,
            correlation_id,
        )

    cdef TradeTick _parse_trade_tick(
        self,
        InstrumentId instrument_id,
        dict trade,
        int price_precision,
        int size_precision,
    ):
        return TradeTick(
            instrument_id,
            Price(trade['price'], price_precision),
            Quantity(trade['amount'], size_precision),
            AggressorSide.BUY if trade["side"] == "buy" else AggressorSide.SELL,
            trade["id"],
            self._ccxt_to_timestamp_ns(millis=trade["timestamp"]),
            self._clock.timestamp_ns(),
        )

    cdef Bar _parse_bar(
        self,
        BarType bar_type,
        list values,
        int price_precision,
        int size_precision,
    ):
        return Bar(
            bar_type,
            Price(values[1], price_precision),
            Price(values[2], price_precision),
            Price(values[3], price_precision),
            Price(values[4], price_precision),
            Quantity(values[5], size_precision),
            self._ccxt_to_timestamp_ns(millis=values[0]),
            self._clock.timestamp_ns(),
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
