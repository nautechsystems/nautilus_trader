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

from betfairlightweight import APIClient, BetfairError

from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.constants cimport *  # str constants only
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport from_unix_time_ms
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.live.data_client cimport LiveMarketDataClient
from nautilus_trader.live.data_engine cimport LiveDataEngine
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport TradeMatchId
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orderbook.book cimport OrderBook
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick

from nautilus_trader.adapters.betfair.common import on_market_update
from nautilus_trader.adapters.betfair.providers cimport BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.sockets import BetfairMarketStreamClient

cdef int _SECONDS_IN_HOUR = 60 * 60

# Notes
# TODO - if you receive con=true flag on a market - then you are consuming data slower than the rate of deliver. If the
#  socket buffer is full we won't attempt to push; so the next push will be conflated.
#  We should warn about this.

# TODO - Betfair reports status:503 in messages if the stream is unhealthy. We should send out a warning / health
#  message, potentially letting strategies know to temporarily "pause" ?

# TODO - segmentationEnabled=true segmentation breaks up large messages and improves: end to end performance, latency,
#  time to first and last byte


cdef class BetfairDataClient(LiveMarketDataClient):
    """
    Provides a data client for the Betfair API.
    """

    def __init__(
        self,
        client not None,
        LiveDataEngine engine not None,
        LiveClock clock not None,
        Logger logger not None,
    ):
        """
        Initialize a new instance of the `BetfairDataClient` class.

        Parameters
        ----------
        client : betfairlightweight.APIClient
            The betfairlightweight client.
        engine : LiveDataEngine
            The live data engine for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.

        Raises
        ------
        """
        super().__init__(
            "BetfairDataClient",
            engine,
            clock,
            logger,
        )

        self._client = client  # type: APIClient
        self._instrument_provider = BetfairInstrumentProvider(
            client=client,
            load_all=False,
        )
        self._stream = BetfairMarketStreamClient(
            client=self._client, message_handler=self._on_market_update,
        )

        self.is_connected = False

        # Subscriptions
        self._subscribed_instruments = set()   # type: set[InstrumentId]
        self._subscribed_markets = {}      # type: dict[InstrumentId, asyncio.Task]

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
    def subscribed_markets(self):
        """
        The quote tick instruments subscribed to.

        Returns
        -------
        list[InstrumentId]

        """
        return sorted(list(self._subscribed_markets.keys()))

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
        # Load & handle instruments
        self._load_instruments()

        for instrument in self._instrument_provider.get_all().values():
            self._handle_instrument(instrument)

        # Start market data stream
        await self._stream.connect()

        self.is_connected = True
        self._log.info("Connected.")

    cpdef void disconnect(self) except *:
        """
        Disconnect the client.
        """
        self._loop.create_task(self._disconnect())

    async def _disconnect(self):
        self._log.info("Disconnecting...")

        stop_tasks = []

        # Cancel update instruments
        if self._update_instruments_task:
            self._update_instruments_task.cancel()
            # TODO: This task is not finishing
            # stop_tasks.append(self._update_instruments_task)

        # Cancel residual tasks
        for task in self._subscribed_markets.values():
            if not task.cancelled():
                self._log.debug(f"Cancelling {task}...")
                task.cancel()

        if stop_tasks:
            await asyncio.gather(*stop_tasks)

        # Ensure client closed
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

        # TODO: Reset client
        self._instrument_provider = BetfairInstrumentProvider(
            client=self._client,
            load_all=False,
        )

        self._subscribed_instruments = set()

        # Check all tasks have been popped and cancelled
        assert len(self._subscribed_markets) == 0

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
        Subscribe to `Instrument` data for the given instrument identifier.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to subscribe to.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._subscribed_instruments.add(instrument_id)

    cpdef void subscribe_markets(
        self,
        list instrument_ids,
        int level,
        int depth=0,
        dict kwargs=None,
    ) except *:
        """
        Subscribe to `OrderBook` data for the given instrument identifier.

        Parameters
        ----------
        instrument_ids : InstrumentId
            A list of instrument ids to subscribe to order books.
        depth : int, optional
            The maximum depth for the order book. A depth of 0 is maximum depth.
        kwargs : dict, optional
            The keyword arguments for exchange specific parameters.

        """
        pass
        # if kwargs is None:
        #     kwargs = {}
        # Condition.not_none(instrument_id, "instrument_id")
        #
        # # TODO - create data socket, send subsribe message. If at max subscription, add more sockets
        #
        # if instrument_id in self._subscribed_order_books:
        #     self._log.warning(f"Already subscribed {instrument_id.symbol} <OrderBook> data.")
        #     return
        #
        # task = self._loop.create_task(self._watch_order_book(
        #     instrument_id=instrument_id,
        #     level=level,
        #     depth=depth,
        #     kwargs=kwargs,
        # ))
        # self._subscribed_order_books[instrument_id] = task
        #
        # self._log.info(f"Subscribed to {instrument_id.symbol} <OrderBook> data.")

    cpdef void unsubscribe_instrument(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `Instrument` data for the given instrument identifier.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to unsubscribe from.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._subscribed_instruments.discard(instrument_id)

    cpdef void unsubscribe_markets(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `OrderBook` data for the given instrument identifier.

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

# -- REQUESTS --------------------------------------------------------------------------------------

    cpdef void request_instrument(self, InstrumentId instrument_id, UUID correlation_id) except *:
        """
        Request the instrument for the given instrument identifier.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the request.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._loop.create_task(self._request_instrument(instrument_id))

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

    # TODO v2.0 Client or Historical client - historical data lives totally separately
    # cpdef void request_trade_ticks(
    #     self,
    #     InstrumentId instrument_id,
    #     datetime from_datetime,
    #     datetime to_datetime,
    #     int limit,
    #     UUID correlation_id,
    # ) except *:
    #     """
    #     Request historical trade ticks for the given parameters.
    #
    #     Parameters
    #     ----------
    #     instrument_id : InstrumentId
    #         The tick instrument identifier for the request.
    #     from_datetime : datetime, optional
    #         The specified from datetime for the data.
    #     to_datetime : datetime, optional
    #         The specified to datetime for the data. If None then will default
    #         to the current datetime.
    #     limit : int
    #         The limit for the number of returned ticks.
    #     correlation_id : UUID
    #         The correlation identifier for the request.
    #
    #     """
    #     Condition.not_none(instrument_id, "instrument_id")
    #     Condition.not_none(correlation_id, "correlation_id")
    #
    #     if to_datetime is not None:
    #         self._log.warning(f"`request_trade_ticks` was called with a `to_datetime` "
    #                           f"argument of {to_datetime} when not supported by the exchange "
    #                           f"(will use `limit` of {limit}).")
    #
    #     self._loop.create_task(self._request_trade_ticks(
    #         instrument_id,
    #         from_datetime,
    #         to_datetime,
    #         limit,
    #         correlation_id,
    #     ))

# -- INTERNAL --------------------------------------------------------------------------------------

    cdef inline void _log_betfair_error(self, ex, str method_name) except *:
        self._log.warning(f"{type(ex).__name__}: {ex} in {method_name}")

# -- Instrument helpers ---------------------------------------------------------

    cpdef BetfairInstrumentProvider instrument_provider(self):
        return self._instrument_provider

# -- STREAMS ---------------------------------------------------------------------------------------

    cpdef _on_market_update(self, dict update):
        return on_market_update(self, update)

    cdef inline void _on_quote_tick(
        self,
        InstrumentId instrument_id,
        double best_bid,
        double best_ask,
        double best_bid_size,
        double best_ask_size,
        long timestamp,
        int price_precision,
        int size_precision,
    ) except *:
        cdef QuoteTick tick = QuoteTick(
            instrument_id,
            Price(best_bid, price_precision),
            Price(best_ask, price_precision),
            Quantity(best_bid_size, size_precision),
            Quantity(best_ask_size, size_precision),
            from_unix_time_ms(timestamp),
        )

        self._handle_quote_tick(tick)

    cdef inline void _on_trade_tick(
        self,
        InstrumentId instrument_id,
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
            instrument_id,
            Price(price, price_precision),
            Quantity(amount, size_precision),
            side,
            TradeMatchId(trade_match_id),
            from_unix_time_ms(timestamp),
        )

        self._handle_trade_tick(tick)

    async def _run_after_delay(self, double delay, coro):
        # TODO - Can we use loop.call_soon here?
        await asyncio.sleep(delay)
        return await coro

    def _load_instruments(self):
        self._instrument_provider.load_all()
        self._log.info(f"Updated {len(self._instrument_provider._instruments)} instruments.")

    async def _request_instrument(self, InstrumentId instrument_id, UUID correlation_id):
        await self._load_instruments()
        cdef Instrument instrument = self._instrument_provider.find_c(instrument_id)
        if instrument is not None:
            self._handle_instruments([instrument], correlation_id)
        else:
            self._log.error(f"Could not find instrument {instrument_id.symbol}.")

    async def _request_instruments(self, correlation_id):
        await self._load_instruments()
        cdef list instruments = list(self._instrument_provider.get_all().values())
        self._handle_instruments(instruments, correlation_id)

    async def _subscribed_instruments_update(self, delay):
        self._log.info("Loading all instruments")
        self._load_instruments()

        cdef InstrumentId instrument_id
        cdef Instrument instrument
        for instrument_id in self._subscribed_instruments:
            instrument = self._instrument_provider.find_c(instrument_id)
            if instrument is not None:
                self._handle_instrument(instrument)
            else:
                self._log.error(f"Could not find instrument {instrument_id.symbol}.")

        # Reschedule subscribed instruments update
        update = self._run_after_delay(delay, self._subscribed_instruments_update(delay))
        self._update_instruments_task = self._loop.create_task(update)
