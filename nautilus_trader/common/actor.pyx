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

"""
The `TradingStrategy` class allows traders to implement their own customized trading strategies.

A user can inherit from `TradingStrategy` and optionally override any of the
"on" named event methods. The class is not entirely initialized in a stand-alone
way, the intended usage is to pass strategies to a `Trader` so that they can be
fully "wired" into the platform. Exceptions will be raised if a `TradingStrategy`
attempts to operate without a managing `Trader` instance.

"""

import warnings

import cython

from cpython.datetime cimport datetime

from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.c_enums.component_state cimport ComponentState
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.logging cimport CMD
from nautilus_trader.common.logging cimport REQ
from nautilus_trader.common.logging cimport SENT
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Event
from nautilus_trader.data.messages cimport DataRequest
from nautilus_trader.data.messages cimport DataResponse
from nautilus_trader.data.messages cimport Subscribe
from nautilus_trader.data.messages cimport Unsubscribe
from nautilus_trader.model.c_enums.book_level cimport BookLevel
from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.bar cimport BarType
from nautilus_trader.model.data.base cimport Data
from nautilus_trader.model.data.base cimport DataType
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.data.venue cimport InstrumentClosePrice
from nautilus_trader.model.data.venue cimport InstrumentStatusUpdate
from nautilus_trader.model.data.venue cimport VenueStatusUpdate
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ComponentId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.orderbook.data cimport OrderBookData
from nautilus_trader.msgbus.bus cimport MessageBus


cdef class Actor(Component):
    """
    The abstract base class for all actor components.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self, ComponentId component_id=None):
        """
        Initialize a new instance of the ``Actor`` class.

        Parameters
        ----------
        component_id : ComponentId, optional
            The component ID. If None is passed then the identifier will be
            taken from `type(self).__name__`.

        """
        cdef Clock clock = LiveClock()
        super().__init__(
            clock=clock,
            logger=Logger(clock=clock),
            component_id=component_id,
            log_initialized=False,
        )

        self.trader_id = None  # Initialized when registered
        self.msgbus = None     # Initialized when registered
        self.cache = None      # Initialized when registered

    cdef void _check_registered(self) except *:
        if self.trader_id is None:
            # This guards the case where some components are called which
            # have not yet been assigned, resulting in a SIGSEGV at runtime.
            raise RuntimeError("Actor has not been registered with a trader")

# -- ABSTRACT METHODS ------------------------------------------------------------------------------

    cpdef void on_start(self) except *:
        """
        Actions to be performed on start.

        The intent is that this method is called once per fresh trading session
        when the component is initially started.

        It is recommended to subscribe/request data here.

        Warnings
        --------
        System method (not intended to be called by user code).

        Should be overridden in the implementation.

        """
        # Should override in subclass
        warnings.warn("on_start was called when not overridden")

    cpdef void on_stop(self) except *:
        """
        Actions to be performed on stopped.

        The intent is that this method is called every time the strategy is
        paused, and also when it is done for day.

        Warnings
        --------
        System method (not intended to be called by user code).

        Should be overridden in the implementation.

        """
        # Should override in subclass
        warnings.warn("on_stop was called when not overridden")

    cpdef void on_resume(self) except *:
        """
        Actions to be performed on resume.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        pass  # Optionally override in subclass

    cpdef void on_reset(self) except *:
        """
        Actions to be performed on reset.

        Warnings
        --------
        System method (not intended to be called by user code).

        Should be overridden in the strategy implementation.

        """
        # Should override in subclass
        warnings.warn("on_reset was called when not overridden")

    cpdef void on_dispose(self) except *:
        """
        Actions to be performed on dispose.

        Cleanup any resources used by the strategy here.

        Warnings
        --------
        System method (not intended to be called by user code).

        Should be overridden in the strategy implementation.

        """
        # Should override in subclass
        warnings.warn("on_dispose was called when not overridden")

    cpdef void on_instrument(self, Instrument instrument) except *:
        """
        Actions to be performed when running and receives an instrument.

        Parameters
        ----------
        instrument : Instrument
            The instrument received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        pass  # Optionally override in subclass

    cpdef void on_order_book(self, OrderBook order_book) except *:
        """
        Actions to be performed when running and receives an order book snapshot.

        Parameters
        ----------
        order_book : OrderBook
            The order book received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        pass  # Optionally override in subclass

    cpdef void on_order_book_delta(self, OrderBookData delta) except *:
        """
        Actions to be performed when running and receives an order book delta.

        Parameters
        ----------
        delta : OrderBookDelta, OrderBookDeltas, OrderBookSnapshot
            The order book delta received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        pass  # Optionally override in subclass

    cpdef void on_quote_tick(self, QuoteTick tick) except *:
        """
        Actions to be performed when running and receives a quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The tick received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        pass  # Optionally override in subclass

    cpdef void on_trade_tick(self, TradeTick tick) except *:
        """
        Actions to be performed when running and receives a trade tick.

        Parameters
        ----------
        tick : TradeTick
            The tick received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        pass  # Optionally override in subclass

    cpdef void on_bar(self, Bar bar) except *:
        """
        Actions to be performed when running and receives a bar.

        Parameters
        ----------
        bar : Bar
            The bar received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        pass  # Optionally override in subclass

    cpdef void on_venue_status_update(self, VenueStatusUpdate update) except *:
        """
        Actions to be performed when running and receives a venue status update.

        Parameters
        ----------
        update : VenueStatusUpdate
            The update received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        pass  # Optionally override in subclass

    cpdef void on_instrument_status_update(self, InstrumentStatusUpdate update) except *:
        """
        Actions to be performed when running and receives an instrument status
        update.

        Parameters
        ----------
        update : InstrumentStatusUpdate
            The update received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        pass  # Optionally override in subclass

    cpdef void on_instrument_close_price(self, InstrumentClosePrice update) except *:
        """
        Actions to be performed when running and receives an instrument close
        price update.

        Parameters
        ----------
        update : InstrumentClosePrice
            The update received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        pass  # Optionally override in subclass

    cpdef void on_data(self, Data data) except *:
        """
        Actions to be performed when running and receives generic data.

        Parameters
        ----------
        data : Data
            The data received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        pass  # Optionally override in subclass

    cpdef void on_event(self, Event event) except *:
        """
        Actions to be performed running and receives an event.

        Parameters
        ----------
        event : Event
            The event received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        pass  # Optionally override in subclass

# -- REGISTRATION ----------------------------------------------------------------------------------

    cpdef void register_base(
        self,
        TraderId trader_id,
        MessageBus msgbus,
        CacheFacade cache,
        Clock clock,
        Logger logger,
    ) except *:
        """
        Register the component with a trader.

        Parameters
        ----------
        trader_id : TraderId
            The trader ID for the strategy.
        msgbus : MessageBus
            The message bus for the strategy.
        cache : CacheFacade
            The read-only cache for the strategy.
        clock : Clock
            The clock for the strategy.
        logger : Logger
            The logger for the strategy.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(trader_id, "trader_id")
        Condition.not_none(msgbus, "msgbus")
        Condition.not_none(cache, "cache")
        Condition.not_none(clock, "clock")
        Condition.not_none(logger, "logger")

        self.trader_id = trader_id

        clock.register_default_handler(self.handle_event)
        self._change_clock(clock)
        self._change_logger(logger)

        self.msgbus = msgbus
        self.cache = cache

# -- ACTION IMPLEMENTATIONS ------------------------------------------------------------------------

    cpdef void _start(self) except *:
        self._check_registered()
        self.on_start()

    cpdef void _stop(self) except *:
        self._check_registered()

        # Clean up clock
        cdef list timer_names = self._clock.timer_names()
        self._clock.cancel_timers()

        cdef str name
        for name in timer_names:
            self._log.info(f"Cancelled Timer(name={name}).")

        self.on_stop()

    cpdef void _resume(self) except *:
        self._check_registered()
        self.on_resume()

    cpdef void _reset(self) except *:
        self._check_registered()
        self.on_reset()

    cpdef void _dispose(self) except *:
        self._check_registered()
        self.on_dispose()

# -- SUBSCRIPTIONS ---------------------------------------------------------------------------------

    cpdef void subscribe_data(self, ClientId client_id, DataType data_type) except *:
        """
        Subscribe to data of the given data type.

        Parameters
        ----------
        client_id : ClientId
            The data client ID.
        data_type : DataType
            The data type to subscribe to.

        """
        Condition.not_none(client_id, "client_id")
        Condition.not_none(data_type, "data_type")

        self.msgbus.subscribe(
            topic=f"data.{data_type}",
            handler=self.handle_data,
        )

        cdef Subscribe command = Subscribe(
            client_id=client_id,
            data_type=data_type,
            command_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void subscribe_strategy_data(
        self,
        type data_type=None,
        StrategyId strategy_id=None,
    ) except *:
        """
        Subscribe to strategy data of the given data type.

        Parameters
        ----------
        data_type : type, optional
            The strategy data type to subscribe to.
        strategy_id : StrategyId, optional
            The strategy ID filter for the subscription.

        """
        Condition.not_none(data_type, "data_type")

        self.msgbus.subscribe(
            topic=f"data.strategy"
                  f".{data_type.__name__ if data_type else '*'}"
                  f".{strategy_id or '*'}",
            handler=self.handle_data,
        )

        self._log.info(
            f"Subscribed to {data_type.__name__} "
            f"strategy data{strategy_id if strategy_id else ''}.",
        )

    cpdef void subscribe_instrument(self, InstrumentId instrument_id) except *:
        """
        Subscribe to update `Instrument` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the subscription.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self.msgbus.subscribe(
            topic=f"data.instrument"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol}",
            handler=self.handle_instrument,
        )

        cdef Subscribe command = Subscribe(
            client_id=ClientId(instrument_id.venue.value),
            data_type=DataType(Instrument, metadata={"instrument_id": instrument_id}),
            command_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void subscribe_instruments(self, Venue venue) except *:
        """
        Subscribe to update `Instrument` data for the given venue.

        Parameters
        ----------
        venue : Venue
            The venue for the subscription.

        """
        Condition.not_none(venue, "venue")

        self.msgbus.subscribe(
            topic=f"data.instrument.{venue}.*",
            handler=self.handle_instrument,
        )

        cdef Subscribe command = Subscribe(
            client_id=ClientId(venue.value),
            data_type=DataType(Instrument),
            command_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void subscribe_order_book_deltas(
        self,
        InstrumentId instrument_id,
        BookLevel level=BookLevel.L2,
        dict kwargs=None,
    ) except *:
        """
        Subscribe to the order book deltas stream, being a snapshot then deltas
        `OrderBookData` for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument ID to subscribe to.
        level : BookLevel
            The order book level (L1, L2, L3).
        kwargs : dict, optional
            The keyword arguments for exchange specific parameters.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self.msgbus.subscribe(
            topic=f"data.book.deltas"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol}",
            handler=self.handle_order_book_delta,
        )

        cdef Subscribe command = Subscribe(
            client_id=ClientId(instrument_id.venue.value),
            data_type=DataType(OrderBookData, metadata={
                "instrument_id": instrument_id,
                "level": level,
                "kwargs": kwargs,
            }),
            command_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void subscribe_order_book_snapshots(
        self,
        InstrumentId instrument_id,
        BookLevel level=BookLevel.L2,
        int depth=0,
        int interval_ms=1000,
        dict kwargs=None,
    ) except *:
        """
        Subscribe to `OrderBook` snapshots for the given instrument ID.

        The `DataEngine` will only maintain one order book for each instrument.
        Because of this - the level, depth and kwargs for the stream will be set
        as per the last subscription request (this will also affect all subscribers).

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument ID to subscribe to.
        level : BookLevel
            The order book level (L1, L2, L3).
        depth : int, optional
            The maximum depth for the order book. A depth of 0 is maximum depth.
        interval_ms : int
            The order book snapshot interval in milliseconds.
        kwargs : dict, optional
            The keyword arguments for exchange specific parameters.

        Raises
        ------
        ValueError
            If depth is negative (< 0).
        ValueError
            If interval_ms is not positive (> 0).

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_negative(depth, "depth")
        Condition.not_negative(interval_ms, "interval_ms")

        self.msgbus.subscribe(
            topic=f"data.book.snapshots"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol}"
                  f".{interval_ms}",
            handler=self.handle_order_book,
        )

        cdef Subscribe command = Subscribe(
            client_id=ClientId(instrument_id.venue.value),
            data_type=DataType(OrderBook, metadata={
                "instrument_id": instrument_id,
                "level": level,
                "depth": depth,
                "interval_ms": interval_ms,
                "kwargs": kwargs,
            }),
            command_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void subscribe_quote_ticks(self, InstrumentId instrument_id) except *:
        """
        Subscribe to streaming `QuoteTick` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to subscribe to.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self.msgbus.subscribe(
            topic=f"data.quotes"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol}",
            handler=self.handle_quote_tick,
        )

        cdef Subscribe command = Subscribe(
            client_id=ClientId(instrument_id.venue.value),
            data_type=DataType(QuoteTick, metadata={"instrument_id": instrument_id}),
            command_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void subscribe_trade_ticks(self, InstrumentId instrument_id) except *:
        """
        Subscribe to streaming `TradeTick` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to subscribe to.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self.msgbus.subscribe(
            topic=f"data.trades"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol}",
            handler=self.handle_trade_tick,
        )

        cdef Subscribe command = Subscribe(
            client_id=ClientId(instrument_id.venue.value),
            data_type=DataType(TradeTick, metadata={"instrument_id": instrument_id}),
            command_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void subscribe_bars(self, BarType bar_type) except *:
        """
        Subscribe to streaming `Bar` data for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to subscribe to.

        """
        Condition.not_none(bar_type, "bar_type")

        self.msgbus.subscribe(
            topic=f"data.bars.{bar_type}",
            handler=self.handle_bar,
        )

        cdef Subscribe command = Subscribe(
            client_id=ClientId(bar_type.instrument_id.venue.value),
            data_type=DataType(Bar, metadata={"bar_type": bar_type}),
            command_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void subscribe_venue_status_updates(self, Venue venue) except *:
        """
        Subscribe to status updates of the given venue.

        Parameters
        ----------
        venue : Venue
            The venue to subscribe to.

        """
        Condition.not_none(venue, "venue")

        self.msgbus.subscribe(
            topic=f"data.venue.status",
            handler=self.handle_venue_status_update,
        )

        cdef Subscribe command = Subscribe(
            client_id=ClientId(venue.value),
            data_type=DataType(VenueStatusUpdate, metadata={"name": venue.value}),
            command_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void subscribe_instrument_status_updates(self, InstrumentId instrument_id) except *:
        """
        Subscribe to status updates of the given instrument id.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to subscribe to status updates for.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self.msgbus.subscribe(
            topic=f"data.venue.status",
            handler=self.handle_instrument_status_update,
        )

        cdef Subscribe command = Subscribe(
            client_id=ClientId(instrument_id.venue.value),
            data_type=DataType(InstrumentStatusUpdate, metadata={"instrument_id": instrument_id}),
            command_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void subscribe_instrument_close_prices(self, InstrumentId instrument_id) except *:
        """
        Subscribe to closing prices for the given instrument id.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to subscribe to status updates for.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self.msgbus.subscribe(
            topic=f"data.venue.close_price.{instrument_id.value}",
            handler=self.handle_instrument_close_price,
        )

        cdef Subscribe command = Subscribe(
            client_id=ClientId(instrument_id.venue.value),
            data_type=DataType(InstrumentClosePrice, metadata={"instrument_id": instrument_id}),
            command_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void unsubscribe_data(self, ClientId client_id, DataType data_type) except *:
        """
        Unsubscribe from data of the given data type.

        Parameters
        ----------
        client_id : ClientId
            The data client ID.
        data_type : DataType
            The data type to unsubscribe from.

        """
        Condition.not_none(client_id, "client_id")
        Condition.not_none(data_type, "data_type")

        self.msgbus.unsubscribe(topic=f"data.{data_type}", handler=self.handle_data)

        cdef Unsubscribe command = Unsubscribe(
            client_id=client_id,
            data_type=data_type,
            command_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void unsubscribe_strategy_data(
        self,
        type data_type,
        StrategyId strategy_id=None,
    ) except *:
        """
        Unsubscribe from strategy data of the given data type.

        Parameters
        ----------
        data_type : type
            The strategy data type to unsubscribe from.
        strategy_id : StrategyId, optional
            The strategy ID filter for the subscription.

        """
        Condition.not_none(data_type, "data_type")

        self.msgbus.unsubscribe(
            topic=f"data.strategy.{data_type.__name__}.{strategy_id or '*'}",
            handler=self.handle_data,
        )

        strategy_id_str = f" for {strategy_id}" if strategy_id else ""
        self._log.info(f"Unsubscribed from {data_type.__name__} strategy data{strategy_id_str}.")

    cpdef void unsubscribe_instruments(self, Venue venue) except *:
        """
        Unsubscribe from update `Instrument` data for the given venue.

        Parameters
        ----------
        venue : Venue
            The venue for the subscription.

        """
        Condition.not_none(venue, "venue")

        self.msgbus.unsubscribe(
            topic=f"data.instrument.{venue}.*",
            handler=self.handle_instrument,
        )

        cdef Unsubscribe command = Unsubscribe(
            client_id=ClientId(venue.value),
            data_type=DataType(Instrument),
            command_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void unsubscribe_instrument(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from update `Instrument` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to unsubscribe from.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self.msgbus.unsubscribe(
            topic=f"data.instrument"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol}",
            handler=self.handle_instrument,
        )

        cdef Unsubscribe command = Unsubscribe(
            client_id=ClientId(instrument_id.venue.value),
            data_type=DataType(Instrument, metadata={"instrument_id": instrument_id}),
            command_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void unsubscribe_order_book_deltas(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe the order book deltas stream for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to subscribe to.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self.msgbus.unsubscribe(
            topic=f"data.book.deltas"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol}",
            handler=self.handle_order_book_delta,
        )

        cdef Unsubscribe command = Unsubscribe(
            client_id=ClientId(instrument_id.venue.value),
            data_type=DataType(OrderBookData, metadata={"instrument_id": instrument_id}),
            command_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void unsubscribe_order_book_snapshots(
        self,
        InstrumentId instrument_id,
        int interval_ms=1000,
    ) except *:
        """
        Unsubscribe from order book snapshots for the given instrument ID.

        The interval must match the previously subscribed interval.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to subscribe to.
        interval_ms : int
            The order book snapshot interval in milliseconds.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self.msgbus.unsubscribe(
            topic=f"data.book.snapshots"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol}"
                  f".{interval_ms}",
            handler=self.handle_order_book,
        )

        cdef Unsubscribe command = Unsubscribe(
            client_id=ClientId(instrument_id.venue.value),
            data_type=DataType(OrderBook, metadata={
                "instrument_id": instrument_id,
                "interval_ms": interval_ms,
            }),
            command_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void unsubscribe_quote_ticks(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from streaming `QuoteTick` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to unsubscribe from.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self.msgbus.unsubscribe(
            topic=f"data.quotes"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol}",
            handler=self.handle_quote_tick,
        )

        cdef Unsubscribe command = Unsubscribe(
            client_id=ClientId(instrument_id.venue.value),
            data_type=DataType(QuoteTick, metadata={"instrument_id": instrument_id}),
            command_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void unsubscribe_trade_ticks(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from streaming `TradeTick` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument ID to unsubscribe from.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self.msgbus.unsubscribe(
            topic=f"data.trades"
                  f".{instrument_id.venue}"
                  f".{instrument_id.symbol}",
            handler=self.handle_trade_tick,
        )

        cdef Unsubscribe command = Unsubscribe(
            client_id=ClientId(instrument_id.venue.value),
            data_type=DataType(TradeTick, metadata={"instrument_id": instrument_id}),
            command_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void unsubscribe_bars(self, BarType bar_type) except *:
        """
        Unsubscribe from streaming `Bar` data for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to unsubscribe from.

        """
        Condition.not_none(bar_type, "bar_type")

        self.msgbus.unsubscribe(
            topic=f"data.bars.{bar_type}",
            handler=self.handle_bar,
        )

        cdef Unsubscribe command = Unsubscribe(
            client_id=ClientId(bar_type.instrument_id.venue.value),
            data_type=DataType(Bar, metadata={"bar_type": bar_type}),
            command_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_cmd(command)
        self._log.info(f"Unsubscribed from {bar_type} bar data.")

    cpdef void publish_data(self, Data data) except *:
        """
        Publish the given data to the message bus.

        Parameters
        ----------
        data : Data
            The data to publish.

        """
        Condition.not_none(data, "data")

        self.msgbus.publish_c(
            topic=f"data.{type(data).__name__}.{type(self).__name__}",
            msg=data,
        )

# -- REQUESTS --------------------------------------------------------------------------------------

    cpdef void request_data(self, ClientId client_id, DataType data_type) except *:
        """
        Request custom data for the given data type from the given data client.

        Parameters
        ----------
        client_id : ClientId
            The data client ID.
        data_type : DataType
            The data type for the request.

        """
        Condition.not_none(client_id, "client_id")
        Condition.not_none(data_type, "data_type")

        cdef DataRequest request = DataRequest(
            client_id=client_id,
            data_type=data_type,
            callback=self._handle_data_response,
            request_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_req(request)

    cpdef void request_quote_ticks(
        self,
        InstrumentId instrument_id,
        datetime from_datetime=None,
        datetime to_datetime=None,
    ) except *:
        """
        Request historical quote ticks for the given parameters.

        If datetimes are `None` then will request the most recent data.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument ID for the request.
        from_datetime : datetime, optional
            The specified from datetime for the data.
        to_datetime : datetime, optional
            The specified to datetime for the data. If None then will default
            to the current datetime.

        Notes
        -----
        Always limited to the tick capacity of the `DataEngine` cache.

        """
        Condition.not_none(instrument_id, "instrument_id")
        if from_datetime is not None and to_datetime is not None:
            Condition.true(from_datetime < to_datetime, "from_datetime was >= to_datetime")

        cdef DataRequest request = DataRequest(
            client_id=ClientId(instrument_id.venue.value),
            data_type=DataType(QuoteTick, metadata={
                "instrument_id": instrument_id,
                "from_datetime": from_datetime,
                "to_datetime": to_datetime,
            }),
            callback=self._handle_quote_ticks_response,
            request_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_req(request)

    cpdef void request_trade_ticks(
        self,
        InstrumentId instrument_id,
        datetime from_datetime=None,
        datetime to_datetime=None,
    ) except *:
        """
        Request historical trade ticks for the given parameters.

        If datetimes are `None` then will request the most recent data.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument ID for the request.
        from_datetime : datetime, optional
            The specified from datetime for the data.
        to_datetime : datetime, optional
            The specified to datetime for the data. If None then will default
            to the current datetime.

        Notes
        -----
        Always limited to the tick capacity of the `DataEngine` cache.

        """
        Condition.not_none(instrument_id, "instrument_id")
        if from_datetime is not None and to_datetime is not None:
            Condition.true(from_datetime < to_datetime, "from_datetime was >= to_datetime")

        cdef DataRequest request = DataRequest(
            client_id=ClientId(instrument_id.venue.value),
            data_type=DataType(TradeTick, metadata={
                "instrument_id": instrument_id,
                "from_datetime": from_datetime,
                "to_datetime": to_datetime,
            }),
            callback=self._handle_trade_ticks_response,
            request_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_req(request)

    cpdef void request_bars(
        self,
        BarType bar_type,
        datetime from_datetime=None,
        datetime to_datetime=None,
    ) except *:
        """
        Request historical bars for the given parameters.

        If datetimes are `None` then will request the most recent data.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the request.
        from_datetime : datetime, optional
            The specified from datetime for the data.
        to_datetime : datetime, optional
            The specified to datetime for the data. If None then will default
            to the current datetime.

        Notes
        -----
        Always limited to the bar capacity of the `DataEngine` cache.

        """
        Condition.not_none(bar_type, "bar_type")
        if from_datetime is not None and to_datetime is not None:
            Condition.true(from_datetime < to_datetime, "from_datetime was >= to_datetime")

        cdef DataRequest request = DataRequest(
            client_id=ClientId(bar_type.instrument_id.venue.value),
            data_type=DataType(Bar, metadata={
                "bar_type": bar_type,
                "from_datetime": from_datetime,
                "to_datetime": to_datetime,
                "limit": self.cache.bar_capacity,
            }),
            callback=self._handle_bars_response,
            request_id=self._uuid_factory.generate(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_data_req(request)

# -- HANDLERS --------------------------------------------------------------------------------------

    cpdef void handle_instrument(self, Instrument instrument) except *:
        """
        Handle the given instrument.

        Calls `on_instrument` if state is `RUNNING`.

        Parameters
        ----------
        instrument : Instrument
            The received instrument.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(instrument, "instrument")

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_instrument(instrument)
            except Exception as ex:
                self._log.exception(ex)
                raise

    cpdef void handle_order_book_delta(self, OrderBookData delta) except *:
        """
        Handle the given order book data.

        Calls `on_order_book_delta` if state is `RUNNING`.

        Parameters
        ----------
        delta : OrderBookDelta, OrderBookDeltas, OrderBookSnapshot
            The order book delta received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(delta, "data")

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_order_book_delta(delta)
            except Exception as ex:
                self._log.exception(ex)
                raise

    cpdef void handle_order_book(self, OrderBook order_book) except *:
        """
        Handle the given order book snapshot.

        Calls `on_order_book` if state is `RUNNING`.

        Parameters
        ----------
        order_book : OrderBook
            The received order book.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(order_book, "order_book")

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_order_book(order_book)
            except Exception as ex:
                self._log.exception(ex)
                raise

    cpdef void handle_quote_tick(self, QuoteTick tick, bint is_historical=False) except *:
        """
        Handle the given tick.

        Calls `on_quote_tick` if state is `RUNNING`.

        Parameters
        ----------
        tick : QuoteTick
            The received tick.
        is_historical : bool
            If tick is historical then it won't be passed to `on_quote_tick`.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(tick, "tick")

        if is_historical:
            return  # Don't pass to on_tick()

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_quote_tick(tick)
            except Exception as ex:
                self._log.exception(ex)
                raise

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cpdef void handle_quote_ticks(self, list ticks) except *:
        """
        Handle the given tick data by handling each tick individually.

        Parameters
        ----------
        ticks : list[QuoteTick]
            The received ticks.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(ticks, "ticks")  # Could be empty

        cdef int length = len(ticks)
        cdef QuoteTick first = ticks[0] if length > 0 else None
        cdef InstrumentId instrument_id = first.instrument_id if first is not None else None

        if length > 0:
            self._log.info(f"Received <QuoteTick[{length}]> data for {instrument_id}.")
        else:
            self._log.warning("Received <QuoteTick[]> data with no ticks.")

        for i in range(length):
            self.handle_quote_tick(ticks[i], is_historical=True)

    cpdef void handle_trade_tick(self, TradeTick tick, bint is_historical=False) except *:
        """
        Handle the given tick.

        Calls `on_trade_tick` if state is `RUNNING`.

        Parameters
        ----------
        tick : TradeTick
            The received trade tick.
        is_historical : bool
            If tick is historical then it won't be passed to `on_trade_tick`.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(tick, "tick")

        if is_historical:
            return  # Don't pass to on_tick()

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_trade_tick(tick)
            except Exception as ex:
                self._log.exception(ex)
                raise

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cpdef void handle_trade_ticks(self, list ticks) except *:
        """
        Handle the given tick data by handling each tick individually.

        Parameters
        ----------
        ticks : list[TradeTick]
            The received ticks.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(ticks, "ticks")  # Could be empty

        cdef int length = len(ticks)
        cdef TradeTick first = ticks[0] if length > 0 else None
        cdef InstrumentId instrument_id = first.instrument_id if first is not None else None

        if length > 0:
            self._log.info(f"Received <TradeTick[{length}]> data for {instrument_id}.")
        else:
            self._log.warning("Received <TradeTick[]> data with no ticks.")

        for i in range(length):
            self.handle_trade_tick(ticks[i], is_historical=True)

    cpdef void handle_bar(self, Bar bar, bint is_historical=False) except *:
        """
        Handle the given bar data.

        Calls `on_bar` if state is `RUNNING`.

        Parameters
        ----------
        bar : Bar
            The bar received.
        is_historical : bool
            If bar is historical then it won't be passed to `on_bar`.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(bar, "bar")

        if is_historical:
            return  # Don't pass to on_bar()

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_bar(bar)
            except Exception as ex:
                self._log.exception(ex)
                raise

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cpdef void handle_bars(self, list bars) except *:
        """
        Handle the given bar data by handling each bar individually.

        Parameters
        ----------
        bars : list[Bar]
            The bars to handle.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(bars, "bars")  # Can be empty

        cdef int length = len(bars)
        cdef Bar first = bars[0] if length > 0 else None
        cdef Bar last = bars[length - 1] if length > 0 else None

        if length > 0:
            self._log.info(f"Received <Bar[{length}]> data for {first.type}.")
        else:
            self._log.error(f"Received <Bar[{length}]> data for unknown bar type.")
            return  # TODO: Strategy shouldn't receive zero bars

        if length > 0 and first.ts_init > last.ts_init:
            raise RuntimeError(f"cannot handle <Bar[{length}]> data: incorrectly sorted")

        for i in range(length):
            self.handle_bar(bars[i], is_historical=True)

    cpdef void handle_venue_status_update(self, VenueStatusUpdate update) except *:
        """
        Handle the given venue status update.

        Calls `on_venue_status_update` if state is `RUNNING`.

        Parameters
        ----------
        update : VenueStatusUpdate
            The received update.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(update, "update")

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_venue_status_update(update)
            except Exception as ex:
                self._log.exception(ex)
                raise

    cpdef void handle_instrument_status_update(self, InstrumentStatusUpdate update) except *:
        """
        Handle the given instrument status update.

        Calls `on_instrument_status_update` if state is `RUNNING`.

        Parameters
        ----------
        update : InstrumentStatusUpdate
            The received update.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(update, "update")

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_instrument_status_update(update)
            except Exception as ex:
                self._log.exception(ex)
                raise

    cpdef void handle_instrument_close_price(self, InstrumentClosePrice update) except *:
        """
        Handle the given instrument close price update.

        Calls `on_instrument_close_price` if .state is `RUNNING`.

        Parameters
        ----------
        update : InstrumentClosePrice
            The received update.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(update, "update")

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_instrument_close_price(update)
            except Exception as ex:
                self._log.exception(ex)
                raise

    cpdef void handle_data(self, Data data) except *:
        """
        Handle the given data.

        Calls `on_data` if state is `RUNNING`.

        Parameters
        ----------
        data : Data
            The received data.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(data, "data")

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_data(data)
            except Exception as ex:
                self._log.exception(ex)
                raise

    cpdef void handle_event(self, Event event) except *:
        """
        Handle the given event.

        Calls `on_event` if state is `RUNNING`.

        Parameters
        ----------
        event : Event
            The received event.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(event, "event")

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_event(event)
            except Exception as ex:
                self._log.exception(ex)
                raise

    cpdef void _handle_data_response(self, DataResponse response) except *:
        self.handle_bars(response.data)

    cpdef void _handle_quote_ticks_response(self, DataResponse response) except *:
        self.handle_quote_ticks(response.data)

    cpdef void _handle_trade_ticks_response(self, DataResponse response) except *:
        self.handle_trade_ticks(response.data)

    cpdef void _handle_bars_response(self, DataResponse response) except *:
        self.handle_bars(response.data)

# -- EGRESS ----------------------------------------------------------------------------------------

    cdef void _send_data_cmd(self, DataCommand command) except *:
        self._check_registered()
        if not self._log.is_bypassed:
            self._log.info(f"{CMD}{SENT} {command}.")
        self.msgbus.send(endpoint="DataEngine.execute", msg=command)

    cdef void _send_data_req(self, DataRequest request) except *:
        self._check_registered()
        if not self._log.is_bypassed:
            self._log.info(f"{REQ}{SENT} {request}.")
        self.msgbus.request(endpoint="DataEngine.request", request=request)
