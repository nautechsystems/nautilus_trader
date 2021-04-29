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

import ccxtpro
import orjson as json
import zmq

from cpython.datetime cimport datetime

from ccxt.base.errors import BaseError as CCXTError

from nautilus_trader.adapters.ccxt.data cimport CCXTDataClient
from nautilus_trader.adapters.ccxt.providers cimport CCXTInstrumentProvider
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.constants cimport *  # str constants only
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport secs_to_nanos
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.live.data_engine cimport LiveDataEngine
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.orderbook_level cimport OrderBookLevel
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport TradeMatchId
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orderbook.book cimport OrderBookSnapshot
from nautilus_trader.model.tick cimport TradeTick
from nautilus_trader.network.zmq cimport context


cdef int _SECONDS_IN_HOUR = 60 * 60

cdef class CryptoFeedDataClient(CCXTDataClient):
    """
    Provides a data client for the unified CCXT Pro API and Crpytofeed.
    """

    def __init__(
        self,
        client not None,
        LiveDataEngine engine not None,
        LiveClock clock not None,
        Logger logger not None,
    ):
        """
        Initialize a new instance of the `UpbitDataClient` class.

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
            ClientId(client.name.upper()),
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

        self._client = client
        self._instrument_provider = CCXTInstrumentProvider(
            client=client,
            load_all=False,
        )

        self._subscriber = context.socket(zmq.SUB)  # type: zmq.Socket

        self.is_connected = False

        # Subscriptions
        self._subscribed_instruments = set()  # type: set[InstrumentId]
        self._subscribed_order_books = {}  # type: dict[InstrumentId, asyncio.Task]
        self._subscribed_quote_ticks = {}  # type: dict[InstrumentId, asyncio.Task]
        self._subscribed_trade_ticks = {}  # type: dict[InstrumentId, asyncio.Task]
        self._subscribed_bars = {}  # type: dict[BarType, asyncio.Task]

        # Caches
        self._market_id_to_instrument = {}

        # ZeroMQ task
        self._handle_messages_task = None

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

        self._subscriber.bind("tcp://127.0.0.1:5678")
        self._handle_messages_task = self._loop.create_task(self._handle_messages())

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

        if not self._subscriber.closed:
            self._subscriber.close()

        if not self._handle_messages_task.cancelled():
            self._handle_messages_task.cancel()

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

        # Check all tasks have been popped and cancelled
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

        self._log.info("Disposed.")

    # -- SUBSCRIPTIONS ---------------------------------------------------------------------------------

    cpdef void subscribe_order_book(
        self,
        InstrumentId instrument_id,
        OrderBookLevel level,
        int depth=0,
        dict kwargs=None,
    ) except *:
        """
        Subscribe to `OrderBook` data for the given instrument identifier.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to subscribe to.
        level : OrderBookLevel (Enum)
            The order book level (L1, L2, L3).
        depth : int, optional
            The maximum depth for the order book. A depth of 0 is maximum depth.
        kwargs : dict, optional
            The keyword arguments for exchange specific parameters.

        """
        if kwargs is None:
            kwargs = {}
        Condition.not_none(instrument_id, "instrument_id")

        if instrument_id in self._subscribed_order_books:
            self._log.warning(f"Already subscribed {instrument_id.symbol} <OrderBook> data.")
            return

        cdef Instrument instrument = self._instrument_provider.find(instrument_id)
        cdef str market_id = self._convert_instrument_to_market_id(instrument)
        cdef str topic = self._make_order_book_topic(instrument)
        self._subscriber.setsockopt_string(zmq.SUBSCRIBE, topic)
        self._market_id_to_instrument[market_id] = instrument

        self._subscribed_order_books[instrument_id] = True
        self._log.info(f"Subscribed to {instrument_id.symbol} <OrderBook> data.")

    cpdef void subscribe_quote_ticks(self, InstrumentId instrument_id) except *:
        """
        Subscribe to `QuoteTick` data for the given instrument identifier.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to subscribe to.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._log.error(f"`subscribe_quote_ticks` was called when not supported by the exchange.")

    cpdef void subscribe_trade_ticks(self, InstrumentId instrument_id) except *:
        """
        Subscribe to `TradeTick` data for the given instrument identifier.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to subscribe to.

        """
        Condition.not_none(instrument_id, "instrument_id")

        if instrument_id in self._subscribed_trade_ticks:
            self._log.warning(f"Already subscribed {instrument_id.symbol} <TradeTick> data.")
            return

        cdef Instrument instrument = self._instrument_provider.find(instrument_id)
        cdef str market_id = self._convert_instrument_to_market_id(instrument)
        cdef str topic = self._make_trade_tick_topic(instrument)
        self._subscriber.setsockopt_string(zmq.SUBSCRIBE, topic)
        self._market_id_to_instrument[market_id] = instrument

        self._subscribed_trade_ticks[instrument_id] = True
        self._log.info(f"Subscribed to {instrument_id.symbol} <TradeTick> data.")

    cpdef void unsubscribe_order_book(self, InstrumentId instrument_id) except *:
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
        self._subscribed_order_books.pop(instrument_id)

        cdef Instrument instrument = self._instrument_provider.find(instrument_id)
        cdef str market_id = self._convert_instrument_to_market_id(instrument)
        cdef str topic = self._make_order_book_topic(instrument)
        self._subscriber.setsockopt_string(zmq.UNSUBSCRIBE, topic)
        self._market_id_to_instrument.pop(market_id)

        self._log.debug(f"Unsubscribe {topic}.")
        self._log.info(f"Unsubscribed from {instrument_id.symbol} <OrderBook> data.")

    cpdef void unsubscribe_quote_ticks(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `QuoteTick` data for the given instrument identifier.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to unsubscribe from.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._log.error(f"`subscribe_quote_ticks` was called when not supported by the exchange.")

    cpdef void unsubscribe_trade_ticks(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `TradeTick` data for the given instrument identifier.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to unsubscribe from.

        """
        Condition.not_none(instrument_id, "instrument_id")

        if instrument_id not in self._subscribed_trade_ticks:
            self._log.debug(f"Not subscribed to {instrument_id.symbol} <TradeTick> data.")
            return
        self._subscribed_trade_ticks.pop(instrument_id)

        cdef Instrument instrument = self._instrument_provider.find(instrument_id)
        cdef str market_id = self._convert_instrument_to_market_id(instrument)
        cdef str topic = self._make_trade_tick_topic(instrument)
        self._subscriber.setsockopt_string(zmq.UNSUBSCRIBE, topic)
        self._market_id_to_instrument.pop(market_id)

        self._log.debug(f"Unsubscribe {topic}.")
        self._log.info(f"Unsubscribed from {instrument_id.symbol} <TradeTick> data.")

    # -- REQUESTS --------------------------------------------------------------------------------------

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
            The tick instrument identifier for the request.
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
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_negative_int(limit, "limit")
        Condition.not_none(correlation_id, "correlation_id")

        self._log.warning("`request_quote_ticks` was called when not supported "
                          "by the exchange.")

    # -- STREAMS ---------------------------------------------------------------------------------------

    async def _handle_messages(self):
        cdef:
            str data
            str key, raw_message
            str venue, message_type, base, quote
            dict message

            Instrument instrument
            OrderBookSnapshot snapshot
            TradeTick trade
            OrderSide side
        try:
            while True:
                # noinspection PyUnresolvedReferences
                data = await self._subscriber.recv_string()
                key, raw_message = data.split(" ", 1)
                venue, message_type, base, quote = key.split("-")
                message = json.loads(raw_message)

                market_id = f"{base}-{quote}"
                instrument = self._market_id_to_instrument.get(market_id, None)
                if not instrument:
                    self._log.debug(f"Unregistered instrument.")
                    continue

                if message_type == "book":
                    # TODO: For development, Delete below ASAP after implemented.
                    """
{'ask': {'1455': 498550.66626991,
         '1460': 752781.35603282,
         '1465': 598228.58882316,
         '1470': 249278.2237729,
         '1475': 282963.98928846,
         '1480': 231634.82544128,
         '1485': 592018.91254835,
         '1490': 933372.46188308,
         '1495': 1474138.23134572,
         '1500': 1772175.25386338,
         '1505': 635643.73391817,
         '1510': 1304402.14643114,
         '1515': 1293246.94080528,
         '1520': 1071104.98404432,
         '1525': 1186129.1912015},
 'bid': {'1380': 1396625.10260371,
         '1385': 1532504.3456328,
         '1390': 1813638.68972936,
         '1395': 2588561.79170858,
         '1400': 2678269.16178589,
         '1405': 1466848.84801902,
         '1410': 1486703.73623219,
         '1415': 599794.03410828,
         '1420': 1127859.5218116,
         '1425': 1037889.68073613,
         '1430': 758289.32125038,
         '1435': 1081831.35356568,
         '1440': 965512.05217219,
         '1445': 954160.38585135,
         '1450': 1049142.33079387},
 'delta': False,
 'receipt_timestamp': 1619454504.2286043,
 'timestamp': 1619454502.563}
                    """
                    snapshot = OrderBookSnapshot(
                        instrument_id=instrument.id,
                        level=OrderBookLevel.L2,
                        bids=[[float(price), float(quantity)] for
                              price, quantity in message['bid'].items()],
                        asks=[[float(price), float(quantity)] for
                              price, quantity in message['ask'].items()],
                        timestamp_ns=secs_to_nanos(message["receipt_timestamp"])
                    )
                    self._handle_data(snapshot)
                elif message_type == "trades":
                    # TODO: For development, Delete below ASAP after implemented.
                    """
{'amount': 1.53392402,
 'feed': 'UPBIT',
 'id': 1619454502000001,
 'order_type': None,
 'price': 9130,
 'receipt_timestamp': 1619454504.7166424,
 'side': 'buy',
 'symbol': 'CBK-KRW',
 'timestamp': 1619454502}
                    """
                    side = OrderSide.BUY if message['side'] == "buy" else OrderSide.SELL
                    trade = TradeTick(
                        instrument_id=instrument.id,
                        price=Price(message['price'], instrument.price_precision),
                        size=Quantity(message['amount'], instrument.size_precision),
                        side=side,
                        match_id=TradeMatchId(str(message["id"])),
                        timestamp_ns=secs_to_nanos(message['receipt_timestamp'])
                    )
                    self._handle_data(trade)
                else:
                    self._log.debug(f"Unrecognized msg_type arrived:{message_type}")
        except asyncio.CancelledError as ex:
            self._log.debug(f"Cancelled `_handle_messages`.")
        except Exception as ex:
            self._log.exception(ex)

    cdef str _convert_instrument_to_market_id(self, Instrument instrument):
        # Convert symbol to market id(needed when using api)
        return f"{instrument.base_currency.code}-{instrument.quote_currency.code}"

    cdef str _make_order_book_topic(self, Instrument instrument):
        """
        Make Topic to subscribe crpytofeed

        Example:
            UPBIT-book-ICX-KRW
        """
        return f"{self._client.name.upper()}-book-" \
               f"{self._convert_instrument_to_market_id(instrument)}"

    cdef str _make_trade_tick_topic(self, Instrument instrument):
        """
        Make Topic to subscribe crpytofeed

        Example:
            UPBIT-trades-ICX-KRW
        """
        return f"{self._client.name.upper()}-trades-" \
               f"{self._convert_instrument_to_market_id(instrument)}"
