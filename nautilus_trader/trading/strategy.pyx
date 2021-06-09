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

from nautilus_trader.common.c_enums.component_state cimport ComponentState
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.component cimport Component
from nautilus_trader.common.factories cimport OrderFactory
from nautilus_trader.common.logging cimport CMD
from nautilus_trader.common.logging cimport EVT
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport RECV
from nautilus_trader.common.logging cimport REQ
from nautilus_trader.common.logging cimport SENT
from nautilus_trader.core.constants cimport *  # str constants only
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.data.engine cimport DataEngine
from nautilus_trader.data.messages cimport DataRequest
from nautilus_trader.data.messages cimport Subscribe
from nautilus_trader.data.messages cimport Unsubscribe
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.orderbook_level cimport OrderBookLevel
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.commands cimport UpdateOrder
from nautilus_trader.model.data cimport Data
from nautilus_trader.model.data cimport DataType
from nautilus_trader.model.events cimport Event
from nautilus_trader.model.events cimport OrderCancelRejected
from nautilus_trader.model.events cimport OrderDenied
from nautilus_trader.model.events cimport OrderInvalid
from nautilus_trader.model.events cimport OrderRejected
from nautilus_trader.model.events cimport OrderUpdateRejected
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orderbook.book cimport OrderBookData
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.base cimport PassiveOrder
from nautilus_trader.model.orders.bracket cimport BracketOrder
from nautilus_trader.model.orders.market cimport MarketOrder
from nautilus_trader.model.position cimport Position
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick
from nautilus_trader.risk.engine cimport RiskEngine


# Events for WRN log level
cdef tuple _WARNING_EVENTS = (
    OrderInvalid,
    OrderDenied,
    OrderRejected,
    OrderCancelRejected,
    OrderUpdateRejected,
)


cdef class TradingStrategy(Component):
    """
    The abstract base class for all trading strategies.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self, str order_id_tag not None):
        """
        Initialize a new instance of the ``TradingStrategy`` class.

        Parameters
        ----------
        order_id_tag : str
            The unique order identifier tag for the strategy. Must be unique
            amongst all running strategies for a particular trader identifier.

        Raises
        ------
        ValueError
            If order_id_tag is not a valid string.

        """
        Condition.valid_string(order_id_tag, "order_id_tag")

        cdef StrategyId strategy_id = StrategyId(f"{type(self).__name__}-{order_id_tag}")
        cdef Clock clock = LiveClock()
        super().__init__(
            clock=clock,
            logger=Logger(clock=clock),
            name=strategy_id.value,
            log_initialized=False,
        )

        self._data_engine = None  # Initialized when registered with the data engine
        self._risk_engine = None  # Initialized when registered with the execution engine

        # Identifiers
        self.trader_id = None     # Initialized when registered with a trader
        self.id = strategy_id

        # Indicators
        self._indicators = []              # type: list[Indicator]
        self._indicators_for_quotes = {}   # type: dict[InstrumentId, list[Indicator]]
        self._indicators_for_trades = {}   # type: dict[InstrumentId, list[Indicator]]
        self._indicators_for_bars = {}     # type: dict[BarType, list[Indicator]]

        # Public components
        self.clock = self._clock
        self.uuid_factory = self._uuid_factory
        self.log = self._log

        self.cache = None          # Initialized when registered with the risk engine
        self.portfolio = None      # Initialized when registered with the risk engine
        self.order_factory = None  # Initialized when registered with a trader

    def __eq__(self, TradingStrategy other) -> bool:
        return self.id.value == other.id.value

    def __ne__(self, TradingStrategy other) -> bool:
        return self.id.value != other.id.value

    cdef void _check_trader_registered(self) except *:
        if self.trader_id is None:
            # This guards the case where some components are called which
            # have not yet been assigned, resulting in a SIGSEGV at runtime.
            raise RuntimeError("the strategy has not been registered with a trader")

    @property
    def registered_indicators(self):
        """
        The registered indicators for the strategy.

        Returns
        -------
        list[Indicator]

        """
        return self._indicators.copy()

    cpdef bint indicators_initialized(self) except *:
        """
        Return a value indicating whether all indicators are initialized.

        Returns
        -------
        bool
            True if all initialized, else False

        """
        if not self._indicators:
            return False

        cdef Indicator indicator
        for indicator in self._indicators:
            if not indicator.initialized:
                return False
        return True

# -- ABSTRACT METHODS ------------------------------------------------------------------------------

    cpdef void on_start(self) except *:
        """
        Actions to be performed on strategy start.

        The intent is that this method is called once per fresh trading session
        when the strategy is initially started.

        It is recommended to subscribe/request data here, and also register
        indicators for data.

        Warnings
        --------
        System method (not intended to be called by user code).

        Should be overridden in the strategy implementation.

        """
        # Should override in subclass
        warnings.warn("on_start was called when not overridden")

    cpdef void on_stop(self) except *:
        """
        Actions to be performed when the strategy is stopped.

        The intent is that this method is called every time the strategy is
        paused, and also when it is done for day.

        Warnings
        --------
        System method (not intended to be called by user code).

        Should be overridden in the strategy implementation.

        """
        # Should override in subclass
        warnings.warn("on_stop was called when not overridden")

    cpdef void on_resume(self) except *:
        """
        Actions to be performed when the strategy is resumed.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        pass  # Optionally override in subclass

    cpdef void on_reset(self) except *:
        """
        Actions to be performed when the strategy is reset.

        Warnings
        --------
        System method (not intended to be called by user code).

        Should be overridden in the strategy implementation.

        """
        # Should override in subclass
        warnings.warn("on_reset was called when not overridden")

    cpdef dict on_save(self):
        """
        Actions to be performed when the strategy is saved.

        Create and return a state dictionary of values to be saved.

        Returns
        -------
        dict[str, bytes]
            The strategy state dictionary.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        return {}  # Optionally override in subclass

    cpdef void on_load(self, dict state) except *:
        """
        Actions to be performed when the strategy is loaded.

        Saved state values will be contained in the give state dictionary.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        pass  # Optionally override in subclass

    cpdef void on_dispose(self) except *:
        """
        Actions to be performed when the strategy is disposed.

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
        Actions to be performed when the strategy is running and receives an
        instrument.

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
        Actions to be performed when the strategy is running and receives an
        order book snapshot.

        Parameters
        ----------
        order_book : OrderBook
            The order book received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        pass  # Optionally override in subclass

    cpdef void on_order_book_delta(self, OrderBookData data) except *:
        """
        Actions to be performed when the strategy is running and receives an
        order book snapshot.

        Parameters
        ----------
        data : OrderBookData
            The order book snapshot / operations received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        pass  # Optionally override in subclass

    cpdef void on_quote_tick(self, QuoteTick tick) except *:
        """
        Actions to be performed when the strategy is running and receives a quote tick.

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
        Actions to be performed when the strategy is running and receives a trade tick.

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
        Actions to be performed when the strategy is running and receives a bar.

        Parameters
        ----------
        bar : Bar
            The bar received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        pass  # Optionally override in subclass

    cpdef void on_data(self, Data data) except *:
        """
        Actions to be performed when the strategy is running and receives generic data.

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
        Actions to be performed when the strategy is running and receives an event.

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

    cpdef void register_trader(
        self,
        TraderId trader_id,
        Clock clock,
        Logger logger,
        int order_id_count=0,
    ) except *:
        """
        Register the strategy with a trader.

        Parameters
        ----------
        trader_id : TraderId
            The trader identifier for the strategy.
        clock : Clock
            The clock for the strategy.
        logger : Logger
            The logger for the strategy.
        order_id_count : int, optional
            The running order identifier count for the strategy.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(trader_id, "trader_id")
        Condition.not_none(clock, "clock")
        Condition.not_none(logger, "logger")

        self.trader_id = trader_id

        clock.register_default_handler(self.handle_event)
        self._change_clock(clock)
        self.clock = self._clock

        self._change_logger(logger)
        self.log = self._log

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=self.id,
            clock=self.clock,
        )

        self.order_factory.set_count(order_id_count)
        self.log.info(f"Set ClientOrderIdGenerator count to {order_id_count}.")

    cpdef void register_data_engine(self, DataEngine engine) except *:
        """
        Register the strategy with the given data engine.

        Parameters
        ----------
        engine : DataEngine
            The data engine to register.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(engine, "engine")

        self._data_engine = engine
        self.cache = engine.cache

    cpdef void register_risk_engine(self, RiskEngine engine) except *:
        """
        Register the strategy with the given risk engine.

        Parameters
        ----------
        engine : RiskEngine
            The risk engine to register.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(engine, "engine")

        self._risk_engine = engine
        self.cache = engine.cache

    cpdef void register_portfolio(self, Portfolio portfolio) except *:
        """
        Register the strategy with the given portfolio.

        Parameters
        ----------
        portfolio : Portfolio
            The portfolio to register.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(portfolio, "portfolio")

        self.portfolio = portfolio  # Assigned as PortfolioFacade

    cpdef void register_indicator_for_quote_ticks(self, InstrumentId instrument_id, Indicator indicator) except *:
        """
        Register the given indicator with the strategy to receive quote tick
        data for the given instrument identifier.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for tick updates.
        indicator : Indicator
            The indicator to register.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(indicator, "indicator")

        if indicator not in self._indicators:
            self._indicators.append(indicator)

        if instrument_id not in self._indicators_for_quotes:
            self._indicators_for_quotes[instrument_id] = []  # type: list[Indicator]

        if indicator not in self._indicators_for_quotes[instrument_id]:
            self._indicators_for_quotes[instrument_id].append(indicator)
            self.log.info(f"Registered indicator {indicator} for {instrument_id} quote ticks.")
        else:
            self.log.error(f"Indicator {indicator} already registered for {instrument_id} quote ticks.")

    cpdef void register_indicator_for_trade_ticks(self, InstrumentId instrument_id, Indicator indicator) except *:
        """
        Register the given indicator with the strategy to receive trade tick
        data for the given instrument identifier.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for tick updates.
        indicator : indicator
            The indicator to register.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(indicator, "indicator")

        if indicator not in self._indicators:
            self._indicators.append(indicator)

        if instrument_id not in self._indicators_for_trades:
            self._indicators_for_trades[instrument_id] = []  # type: list[Indicator]

        if indicator not in self._indicators_for_trades[instrument_id]:
            self._indicators_for_trades[instrument_id].append(indicator)
            self.log.info(f"Registered indicator {indicator} for {instrument_id} trade ticks.")
        else:
            self.log.error(f"Indicator {indicator} already registered for {instrument_id} trade ticks.")

    cpdef void register_indicator_for_bars(self, BarType bar_type, Indicator indicator) except *:
        """
        Register the given indicator with the strategy to receive bar data for the
        given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type for bar updates.
        indicator : Indicator
            The indicator to register.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(indicator, "indicator")

        if indicator not in self._indicators:
            self._indicators.append(indicator)

        if bar_type not in self._indicators_for_bars:
            self._indicators_for_bars[bar_type] = []  # type: list[Indicator]

        if indicator not in self._indicators_for_bars[bar_type]:
            self._indicators_for_bars[bar_type].append(indicator)
            self.log.info(f"Registered indicator {indicator} for {bar_type} bars.")
        else:
            self.log.error(f"Indicator {indicator} already registered for {bar_type} bars.")

# -- ACTION IMPLEMENTATIONS ------------------------------------------------------------------------

    cpdef void _start(self) except *:
        self._check_trader_registered()
        self.on_start()

    cpdef void _stop(self) except *:
        self._check_trader_registered()

        # Clean up clock
        cdef list timer_names = self.clock.timer_names()
        self.clock.cancel_timers()

        cdef str name
        for name in timer_names:
            self.log.info(f"Cancelled Timer(name={name}).")

        self.on_stop()

    cpdef void _resume(self) except *:
        self._check_trader_registered()
        self.on_resume()

    cpdef void _reset(self) except *:
        self._check_trader_registered()

        if self.order_factory:
            self.order_factory.reset()

        self._indicators.clear()
        self._indicators_for_quotes.clear()
        self._indicators_for_trades.clear()
        self._indicators_for_bars.clear()

        self.on_reset()

    cpdef void _dispose(self) except *:
        self._check_trader_registered()
        self.on_dispose()

# -- STRATEGY COMMANDS -----------------------------------------------------------------------------

    cpdef dict save(self):
        """
        Return the strategy state dictionary to be saved.

        Calls `on_save`.

        Raises
        ------
        RuntimeError
            If strategy is not registered with a trader.

        Warnings
        --------
        Exceptions raised will be caught, logged, and reraised.

        """
        self._check_trader_registered()

        try:
            self.log.debug("Saving state...")
            user_state = self.on_save()
            if len(user_state) > 0:
                self.log.info(f"Saved state: {user_state}.", color=LogColor.BLUE)
            else:
                self.log.info("No user state to save.", color=LogColor.BLUE)
            return user_state
        except Exception as ex:
            self.log.exception(ex)
            raise  # Otherwise invalid state information could be saved

    cpdef void load(self, dict state) except *:
        """
        Load the strategy state from the give state dictionary.

        Calls `on_load` and passes the state.

        Parameters
        ----------
        state : dict[str, object]
            The state dictionary.

        Raises
        ------
        RuntimeError
            If strategy is not registered with a trader.

        Warnings
        --------
        Exceptions raised will be caught, logged, and reraised.

        """
        Condition.not_none(state, "state")

        self._check_trader_registered()

        if not state:
            self.log.info("No user state to load.", color=LogColor.BLUE)
            return

        try:
            self.log.debug(f"Loading state...")
            self.on_load(state)
            self.log.info(f"Loaded state {state}.", color=LogColor.BLUE)
        except Exception as ex:
            self.log.exception(ex)
            raise

# -- SUBSCRIPTIONS ---------------------------------------------------------------------------------

    cpdef void subscribe_data(self, ClientId client_id, DataType data_type) except *:
        """
        Subscribe to data of the given data type.

        Parameters
        ----------
        client_id : ClientId
            The data client identifier.
        data_type : DataType
            The data type to subscribe to.

        """
        Condition.not_none(client_id, "client_id")
        Condition.not_none(self._data_engine, "self._data_engine")

        cdef Subscribe command = Subscribe(
            client_id=client_id,
            data_type=data_type,
            handler=self.handle_data,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void subscribe_instrument(self, InstrumentId instrument_id) except *:
        """
        Subscribe to update `Instrument` data for the given instrument identifier.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier to subscribe to.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(self._data_engine, "data_engine")

        cdef Subscribe command = Subscribe(
            client_id=ClientId(instrument_id.venue.value),
            data_type=DataType(Instrument, metadata={INSTRUMENT_ID: instrument_id}),
            handler=self.handle_instrument,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void subscribe_order_book(
        self,
        InstrumentId instrument_id,
        OrderBookLevel level=OrderBookLevel.L2,
        int depth=0,
        int interval=0,
        dict kwargs=None,
    ) except *:
        """
        Subscribe to streaming `OrderBook` for the given instrument identifier.

        The `DataEngine` will only maintain one order book stream for each
        instrument. Because of this the level, depth and kwargs for the stream will
        be as per the last subscription request (this will also affect all
        subscribers).

        If interval is not specified then will receive every order book update.
        Alternatively specify periodic snapshot intervals in seconds.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument identifier to subscribe to.
        level : OrderBookLevel
            The order book level (L1, L2, L3).
        depth : int, optional
            The maximum depth for the order book. A depth of 0 is maximum depth.
        interval : int, optional
            The order book snapshot interval in seconds.
        kwargs : dict, optional
            The keyword arguments for exchange specific parameters.

        Raises
        ------
        ValueError
            If depth is negative.
        ValueError
            If delay is not None and interval is None.

        """
        Condition.not_none(self._data_engine, "self._data_engine")
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_negative(depth, "depth")
        Condition.not_negative(interval, "interval")

        cdef Subscribe command = Subscribe(
            client_id=ClientId(instrument_id.venue.value),
            data_type=DataType(OrderBook, metadata={
                INSTRUMENT_ID: instrument_id,
                LEVEL: level,
                DEPTH: depth,
                INTERVAL: interval,
                KWARGS: kwargs,
            }),
            handler=self.handle_order_book,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )
        self._send_data_cmd(command)

    cpdef void subscribe_order_book_deltas(
        self,
        InstrumentId instrument_id,
        OrderBookLevel level=OrderBookLevel.L2,
        dict kwargs=None,
    ) except *:
        """
        Subscribe to streaming `OrderBook` snapshot then deltas data for the
        given instrument identifier.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument identifier to subscribe to.
        level : OrderBookLevel
            The order book level (L1, L2, L3).
        kwargs : dict, optional
            The keyword arguments for exchange specific parameters.

        Raises
        ------
        ValueError
            If depth is negative.
        ValueError
            If delay is not None and interval is None.

        """
        Condition.not_none(self._data_engine, "self._data_engine")
        Condition.not_none(instrument_id, "instrument_id")

        cdef Subscribe command = Subscribe(
            client_id=ClientId(instrument_id.venue.value),
            data_type=DataType(OrderBookData, metadata={
                INSTRUMENT_ID: instrument_id,
                LEVEL: level,
                KWARGS: kwargs,
            }),
            handler=self.handle_order_book_delta,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void subscribe_quote_ticks(self, InstrumentId instrument_id) except *:
        """
        Subscribe to streaming `QuoteTick` data for the given instrument identifier.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to subscribe to.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(self._data_engine, "self._data_engine")

        cdef Subscribe command = Subscribe(
            client_id=ClientId(instrument_id.venue.value),
            data_type=DataType(QuoteTick, metadata={INSTRUMENT_ID: instrument_id}),
            handler=self.handle_quote_tick,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void subscribe_trade_ticks(self, InstrumentId instrument_id) except *:
        """
        Subscribe to streaming `TradeTick` data for the given instrument identifier.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to subscribe to.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(self._data_engine, "data_engine")

        cdef Subscribe command = Subscribe(
            client_id=ClientId(instrument_id.venue.value),
            data_type=DataType(TradeTick, metadata={INSTRUMENT_ID: instrument_id}),
            handler=self.handle_trade_tick,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
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
        Condition.not_none(self._data_engine, "self._data_engine")

        cdef Subscribe command = Subscribe(
            client_id=ClientId(bar_type.instrument_id.venue.value),
            data_type=DataType(Bar, metadata={BAR_TYPE: bar_type}),
            handler=self.handle_bar,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void unsubscribe_data(self, ClientId client_id, DataType data_type) except *:
        """
        Unsubscribe from data of the given data type.

        Parameters
        ----------
        client_id : ClientId
            The data client identifier.
        data_type : DataType
            The data type to unsubscribe from.

        """
        Condition.not_none(client_id, "client_id")
        Condition.not_none(self._data_engine, "self._data_engine")

        cdef Unsubscribe command = Unsubscribe(
            client_id=client_id,
            data_type=data_type,
            handler=self.handle_data,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void unsubscribe_instrument(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from update `Instrument` data for the given instrument identifier.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to unsubscribe from.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(self._data_engine, "self._data_engine")

        cdef Unsubscribe command = Unsubscribe(
            client_id=ClientId(instrument_id.venue.value),
            data_type=DataType(Instrument, metadata={INSTRUMENT_ID: instrument_id}),
            handler=self.handle_instrument,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void unsubscribe_order_book(self, InstrumentId instrument_id, int interval=0) except *:
        """
        Unsubscribe from `OrderBook` data for the given instrument identifier.

        The interval must match the previously defined interval if unsubscribing
        from snapshots.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to subscribe to.
        interval : int, optional
            The order book snapshot interval in seconds.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(self._data_engine, "self._data_engine")

        cdef Unsubscribe command = Unsubscribe(
            client_id=ClientId(instrument_id.venue.value),
            data_type=DataType(OrderBook, metadata={
                INSTRUMENT_ID: instrument_id,
                INTERVAL: interval,
            }),
            handler=self.handle_order_book,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void unsubscribe_order_book_deltas(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `OrderBook` data for the given instrument identifier.

        The interval must match the previously defined interval if unsubscribing
        from snapshots.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to subscribe to.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(self._data_engine, "self._data_engine")

        cdef Unsubscribe command = Unsubscribe(
            client_id=ClientId(instrument_id.venue.value),
            data_type=DataType(OrderBookData, metadata={
                INSTRUMENT_ID: instrument_id,
            }),
            handler=self.handle_order_book,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void unsubscribe_quote_ticks(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from streaming `QuoteTick` data for the given instrument identifier.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to unsubscribe from.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(self._data_engine, "self._data_engine")

        cdef Unsubscribe command = Unsubscribe(
            client_id=ClientId(instrument_id.venue.value),
            data_type=DataType(QuoteTick, metadata={INSTRUMENT_ID: instrument_id}),
            handler=self.handle_quote_tick,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

    cpdef void unsubscribe_trade_ticks(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from streaming `TradeTick` data for the given instrument identifier.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument identifier to unsubscribe from.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(self._data_engine, "data_engine")

        cdef Unsubscribe command = Unsubscribe(
            client_id=ClientId(instrument_id.venue.value),
            data_type=DataType(TradeTick, metadata={INSTRUMENT_ID: instrument_id}),
            handler=self.handle_trade_tick,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
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
        Condition.not_none(self._data_engine, "data_engine")

        cdef Unsubscribe command = Unsubscribe(
            client_id=ClientId(bar_type.instrument_id.venue.value),
            data_type=DataType(Bar, metadata={BAR_TYPE: bar_type}),
            handler=self.handle_bar,
            command_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self._send_data_cmd(command)

# -- REQUESTS --------------------------------------------------------------------------------------

    cpdef void request_data(self, ClientId client_id, DataType data_type) except *:
        """
        Request custom data for the given data type from the given data client.

        Parameters
        ----------
        client_id : ClientId
            The data client identifier.
        data_type : DataType
            The data type for the request.

        """
        Condition.not_none(client_id, "client_id")
        Condition.not_none(self._data_engine, "data_engine")

        cdef DataRequest request = DataRequest(
            client_id=client_id,
            data_type=data_type,
            callback=self.handle_data,
            request_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
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
            The tick instrument identifier for the request.
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
        Condition.not_none(self._data_engine, "data_engine")
        if from_datetime is not None and to_datetime is not None:
            Condition.true(from_datetime < to_datetime, "from_datetime was >= to_datetime")

        cdef DataRequest request = DataRequest(
            client_id=ClientId(instrument_id.venue.value),
            data_type=DataType(QuoteTick, metadata={
                INSTRUMENT_ID: instrument_id,
                FROM_DATETIME: from_datetime,
                TO_DATETIME: to_datetime,
                LIMIT: self._data_engine.cache.tick_capacity,
            }),
            callback=self.handle_quote_ticks,
            request_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
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
            The tick instrument identifier for the request.
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
        Condition.not_none(self._data_engine, "data_engine")
        if from_datetime is not None and to_datetime is not None:
            Condition.true(from_datetime < to_datetime, "from_datetime was >= to_datetime")

        cdef DataRequest request = DataRequest(
            client_id=ClientId(instrument_id.venue.value),
            data_type=DataType(TradeTick, metadata={
                INSTRUMENT_ID: instrument_id,
                FROM_DATETIME: from_datetime,
                TO_DATETIME: to_datetime,
                LIMIT: self._data_engine.cache.tick_capacity,
            }),
            callback=self.handle_trade_ticks,
            request_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
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
        Condition.not_none(self._data_engine, "data_engine")
        if from_datetime is not None and to_datetime is not None:
            Condition.true(from_datetime < to_datetime, "from_datetime was >= to_datetime")

        cdef DataRequest request = DataRequest(
            client_id=ClientId(bar_type.instrument_id.venue.value),
            data_type=DataType(Bar, metadata={
                BAR_TYPE: bar_type,
                FROM_DATETIME: from_datetime,
                TO_DATETIME: to_datetime,
                LIMIT: self._data_engine.cache.bar_capacity,
            }),
            callback=self.handle_bars,
            request_id=self.uuid_factory.generate(),
            timestamp_ns=self.clock.timestamp_ns(),
        )

        self._send_data_req(request)

# -- TRADING COMMANDS ------------------------------------------------------------------------------

    cpdef void submit_order(
        self,
        Order order,
        PositionId position_id=None,
    ) except *:
        """
        Submit the given order with optional position identifier and routing instructions.

        A `SubmitOrder` command will be created and then sent to the
        `ExecutionEngine`.

        Parameters
        ----------
        order : Order
            The order to submit.
        position_id : PositionId, optional
            The position identifier to submit the order against.

        """
        Condition.not_none(order, "order")
        Condition.not_none(self.trader_id, "self.trader_id")
        Condition.not_none(self._risk_engine, "self._risk_engine")

        cdef SubmitOrder command = SubmitOrder(
            self.trader_id,
            self.id,
            position_id if position_id is not None else PositionId.null_c(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self._send_exec_cmd(command)

    cpdef void submit_bracket_order(self, BracketOrder bracket_order) except *:
        """
        Submit the given bracket order with optional routing instructions.

        A `SubmitBracketOrder` command with be created and sent to the
        `ExecutionEngine`.

        Parameters
        ----------
        bracket_order : BracketOrder
            The bracket order to submit.

        """
        Condition.not_none(bracket_order, "bracket_order")
        Condition.not_none(self.trader_id, "self.trader_id")
        Condition.not_none(self._risk_engine, "self._risk_engine")

        cdef SubmitBracketOrder command = SubmitBracketOrder(
            self.trader_id,
            self.id,
            bracket_order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self._send_exec_cmd(command)

    cpdef void update_order(
        self,
        PassiveOrder order,
        Quantity quantity=None,
        Price price=None,
        Price trigger=None,
    ) except *:
        """
        Update the given order with optional parameters and routing instructions.

        An `UpdateOrder` command is created and then sent to the
        `ExecutionEngine`. Either one or both values must differ from the
        original order for the command to be valid.

        Will use an Order Cancel/Replace Request (a.k.a Order Modification)
        for FIX protocols, otherwise if order update is not available with
        the API, then will cancel - then replace with a new order using the
        original `ClientOrderId`.

        Parameters
        ----------
        order : PassiveOrder
            The order to update.
        quantity : Quantity, optional
            The updated quantity for the given order.
        price : Price, optional
            The updated price for the given order.
        trigger : Price, optional
            The updated trigger price for the given order.

        Raises
        ------
        ValueError
            If trigger is not None and order.type != STOP_LIMIT

        References
        ----------
        https://www.onixs.biz/fix-dictionary/5.0.SP2/msgType_G_71.html

        """
        Condition.not_none(order, "order")
        Condition.not_none(self.trader_id, "self.trader_id")
        Condition.not_none(self._risk_engine, "self._risk_engine")
        if trigger is not None:
            Condition.equal(order.type, OrderType.STOP_LIMIT, "order.type", "STOP_LIMIT")

        cdef bint updating = False  # Set validation flag (must become true)

        if quantity is not None and quantity != order.quantity:
            updating = True
        else:
            quantity = order.quantity

        if price is not None and price != order.price:
            updating = True
        else:
            price = order.price

        if trigger is not None:
            if order.is_triggered:
                self.log.warning(f"Cannot update order: "
                                 f"{repr(order.client_order_id)} already triggered.")
                return
            if trigger != order.trigger:
                updating = True

        if not updating:
            self.log.error(
                "Cannot create command UpdateOrder "
                "(both quantity and price were None)."
            )
            return

        if order.account_id is None:
            self.log.error(f"Cannot update order: "
                           f"no account assigned to order yet, {order}.")
            return  # Cannot send command

        cdef UpdateOrder command = UpdateOrder(
            self.trader_id,
            self.id,
            order.instrument_id,
            order.client_order_id,
            order.venue_order_id,
            quantity,
            price,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self._send_exec_cmd(command)

    cpdef void cancel_order(self, Order order) except *:
        """
        Cancel the given order with optional routing instructions.

        A `CancelOrder` command will be created and then sent to the
        `ExecutionEngine`.

        Logs an error if no `VenueOrderId` has been assigned to the order.

        Parameters
        ----------
        order : Order
            The order to cancel.

        """
        Condition.not_none(order, "order")
        Condition.not_none(self.trader_id, "self.trader_id")
        Condition.not_none(self._risk_engine, "self._risk_engine")

        if order.venue_order_id.is_null():
            self.log.error(
                f"Cannot cancel order (no venue_order_id assigned yet), "
                f"{order}.",
            )
            return  # Cannot send command

        cdef CancelOrder command = CancelOrder(
            self.trader_id,
            self.id,
            order.instrument_id,
            order.client_order_id,
            order.venue_order_id,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self._send_exec_cmd(command)

    cpdef void cancel_all_orders(self, InstrumentId instrument_id) except *:
        """
        Cancel all orders for this strategy for the given instrument identifier.

        All working orders in turn will have a `CancelOrder` command created and
        then sent to the `ExecutionEngine`.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument for the orders to cancel.

        """
        # instrument_id can be None
        Condition.not_none(self._risk_engine, "self._risk_engine")

        cdef list working_orders = self.cache.orders_working(
            venue=None,  # Faster query filtering
            instrument_id=instrument_id,
            strategy_id=self.id,
        )

        if not working_orders:
            self.log.info("No working orders to cancel.")
            return

        cdef int count = len(working_orders)
        self.log.info(
            f"Cancelling {count} working order{'' if count == 1 else 's'}...",
        )

        cdef Order order
        for order in working_orders:
            self.cancel_order(order)

    cpdef void flatten_position(self, Position position) except *:
        """
        Flatten the given position.

        A closing `MarketOrder` for the position will be created, and then sent
        to the `ExecutionEngine` via a `SubmitOrder` command.

        Parameters
        ----------
        position : Position
            The position to flatten.

        """
        Condition.not_none(position, "position")
        Condition.not_none(self.trader_id, "self.trader_id")
        Condition.not_none(self.order_factory, "self.order_factory")
        Condition.not_none(self._risk_engine, "self._risk_engine")

        if position.is_closed_c():
            self.log.warning(
                f"Cannot flatten position "
                f"(the position is already closed), {position}."
            )
            return  # Invalid command

        # Create flattening order
        cdef MarketOrder order = self.order_factory.market(
            position.instrument_id,
            Order.flatten_side_c(position.side),
            position.quantity,
        )

        # Create command
        cdef SubmitOrder command = SubmitOrder(
            self.trader_id,
            self.id,
            position.id,
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self._send_exec_cmd(command)

    cpdef void flatten_all_positions(self, InstrumentId instrument_id) except *:
        """
        Flatten all positions for the given instrument identifier for this strategy.

        All open positions in turn will have a closing `MarketOrder` created and
        then sent to the `ExecutionEngine` via `SubmitOrder` commands.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument for the positions to flatten.

        """
        # instrument_id can be None
        Condition.not_none(self._risk_engine, "self._risk_engine")

        cdef list positions_open = self.cache.positions_open(
            venue=None,  # Faster query filtering
            instrument_id=instrument_id,
            strategy_id=self.id,
        )

        if not positions_open:
            self.log.info("No open positions to flatten.")
            return

        cdef int count = len(positions_open)
        self.log.info(f"Flattening {count} open position{'' if count == 1 else 's'}...")

        cdef Position position
        for position in positions_open:
            self.flatten_position(position)

# -- HANDLERS --------------------------------------------------------------------------------------

    cpdef void handle_instrument(self, Instrument instrument) except *:
        """
        Handle the given instrument.

        Calls `on_instrument` if `strategy.state` is `RUNNING`.

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
                self.log.exception(ex)
                raise

    cpdef void handle_order_book(self, OrderBook order_book) except *:
        """
        Handle the given order book snapshot.

        Calls `on_order_book` if `strategy.state` is `RUNNING`.

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
                self.log.exception(ex)
                raise

    cpdef void handle_order_book_delta(self, OrderBookData data) except *:
        """
        Handle the given order book snapshot.

        Calls `on_order_book_delta` if `strategy.state` is `RUNNING`.

        Parameters
        ----------
        data : OrderBookData
            The received order book data.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(data, "data")

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_order_book_delta(data)
            except Exception as ex:
                self.log.exception(ex)
                raise

    cpdef void handle_quote_tick(self, QuoteTick tick, bint is_historical=False) except *:
        """
        Handle the given tick.

        Calls `on_quote_tick` if `strategy.state` is `RUNNING`.

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

        # Update indicators
        cdef list indicators = self._indicators_for_quotes.get(tick.instrument_id)  # Could be None
        cdef Indicator indicator
        if indicators:
            for indicator in indicators:
                indicator.handle_quote_tick(tick)

        if is_historical:
            return  # Don't pass to on_tick()

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_quote_tick(tick)
            except Exception as ex:
                self.log.exception(ex)
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
            self.log.info(f"Received <QuoteTick[{length}]> data for {instrument_id}.")
        else:
            self.log.warning("Received <QuoteTick[]> data with no ticks.")

        for i in range(length):
            self.handle_quote_tick(ticks[i], is_historical=True)

    cpdef void handle_trade_tick(self, TradeTick tick, bint is_historical=False) except *:
        """
        Handle the given tick.

        Calls `on_trade_tick` if `strategy.state` is `RUNNING`.

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

        # Update indicators
        cdef list indicators = self._indicators_for_trades.get(tick.instrument_id)  # Could be None
        cdef Indicator indicator
        if indicators:
            for indicator in indicators:
                indicator.handle_trade_tick(tick)

        if is_historical:
            return  # Don't pass to on_tick()

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_trade_tick(tick)
            except Exception as ex:
                self.log.exception(ex)
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
            self.log.info(f"Received <TradeTick[{length}]> data for {instrument_id}.")
        else:
            self.log.warning("Received <TradeTick[]> data with no ticks.")

        for i in range(length):
            self.handle_trade_tick(ticks[i], is_historical=True)

    cpdef void handle_bar(self, Bar bar, bint is_historical=False) except *:
        """
        Handle the given bar data.

        Calls `on_bar` if `strategy.state` is `RUNNING`.

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

        # Update indicators
        cdef list indicators = self._indicators_for_bars.get(bar.type)
        cdef Indicator indicator
        if indicators:
            for indicator in indicators:
                indicator.handle_bar(bar)

        if is_historical:
            return  # Don't pass to on_bar()

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_bar(bar)
            except Exception as ex:
                self.log.exception(ex)
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
            self.log.info(f"Received <Bar[{length}]> data for {first.type}.")
        else:
            self.log.error(f"Received <Bar[{length}]> data for unknown bar type.")
            return  # TODO: Strategy shouldn't receive zero bars

        if length > 0 and first.ts_recv_ns > last.ts_recv_ns:
            raise RuntimeError(f"Cannot handle <Bar[{length}]> data: incorrectly sorted")

        for i in range(length):
            self.handle_bar(bars[i], is_historical=True)

    cpdef void handle_data(self, Data data) except *:
        """
        Handle the given data.

        Calls `on_data` if `strategy.state` is `RUNNING`.

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
                self.log.exception(ex)
                raise

    cpdef void handle_event(self, Event event) except *:
        """
        Handle the given event.

        Calls `on_event` if `strategy.state` is `RUNNING`.

        Parameters
        ----------
        event : Event
            The received event.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(event, "event")

        if isinstance(event, _WARNING_EVENTS):
            self.log.warning(f"{RECV}{EVT} {event}.")
        else:
            self.log.info(f"{RECV}{EVT} {event}.")

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_event(event)
            except Exception as ex:
                self.log.exception(ex)
                raise

# -- INTERNAL --------------------------------------------------------------------------------------

    cdef void _send_data_cmd(self, DataCommand command) except *:
        if not self.log.is_bypassed:
            self.log.info(f"{CMD}{SENT} {command}.")
        self._data_engine.execute(command)

    cdef void _send_data_req(self, DataRequest request) except *:
        if not self.log.is_bypassed:
            self.log.info(f"{REQ}{SENT} {request}.")
        self._data_engine.send(request)

    cdef void _send_exec_cmd(self, TradingCommand command) except *:
        if not self.log.is_bypassed:
            self.log.info(f"{CMD}{SENT} {command}.")
        self._risk_engine.execute(command)
