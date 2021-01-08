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
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport RECV
from nautilus_trader.common.logging cimport SENT
from nautilus_trader.common.logging cimport LiveLogger
from nautilus_trader.common.messages cimport DataRequest
from nautilus_trader.common.messages cimport Subscribe
from nautilus_trader.common.messages cimport Unsubscribe
from nautilus_trader.core.constants cimport *  # str constants only
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.data.engine cimport DataEngine
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport ModifyOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.events cimport Event
from nautilus_trader.model.events cimport OrderCancelReject
from nautilus_trader.model.events cimport OrderRejected
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.order cimport BracketOrder
from nautilus_trader.model.order cimport MarketOrder
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.order cimport PassiveOrder
from nautilus_trader.model.position cimport Position
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick


cdef class TradingStrategy(Component):
    """
    The abstract base class for all trading strategies.

    This class should not be used directly, but through its concrete subclasses.
    """

    def __init__(self, str order_id_tag not None):
        """
        Initialize a new instance of the `TradingStrategy` class.

        Parameters
        ----------
        order_id_tag : str
            The order_id tag for the strategy (must be unique at trader level).

        Raises
        ------
        ValueError
            If order_id_tag is not a valid string.

        """
        Condition.valid_string(order_id_tag, "order_id_tag")

        cdef StrategyId strategy_id = StrategyId(type(self).__name__, order_id_tag)
        cdef Clock clock = LiveClock()
        super().__init__(
            clock=clock,
            logger=LiveLogger(clock, strategy_id.value),
            name=strategy_id.value,
            log_initialized=False,
        )

        self._data_engine = None    # Initialized when registered with the data engine
        self._exec_engine = None    # Initialized when registered with the execution engine

        # Identifiers
        self.trader_id = None       # Initialized when registered with a trader
        self.id = strategy_id

        # Indicators
        self._indicators = []              # type: list[Indicator]
        self._indicators_for_quotes = {}   # type: dict[Symbol, list[Indicator]]
        self._indicators_for_trades = {}   # type: dict[Symbol, list[Indicator]]
        self._indicators_for_bars = {}     # type: dict[BarType, list[Indicator]]

        # Public components
        self.clock = self._clock
        self.uuid_factory = self._uuid_factory
        self.log = self._log

        self.data = None           # Initialized when registered with the data engine
        self.execution = None      # Initialized when registered with the execution engine
        self.portfolio = None      # Initialized when registered with the execution engine
        self.order_factory = None  # Initialized when registered with a trader

    def __eq__(self, TradingStrategy other) -> bool:
        return self.id.value == other.id.value

    def __ne__(self, TradingStrategy other) -> bool:
        return self.id.value != other.id.value

    def __repr__(self) -> str:
        return f"{type(self).__name__}(id={self.id.value})"

    cdef inline void _check_trader_registered(self) except *:
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

        Notes
        -----
        'OrderIdCount' and 'PositionIdCount' are reserved keys for
        the returned state dictionary.

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

    cpdef void on_bar(self, BarType bar_type, Bar bar) except *:
        """
        Actions to be performed when the strategy is running and receives a bar.

        Parameters
        ----------
        bar_type : BarType
            The bar type received.
        bar : Bar
            The bar received.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        pass  # Optionally override in subclass

    cpdef void on_data(self, data) except *:
        """
        Actions to be performed when the strategy is running and receives a data object.

        Parameters
        ----------
        data : object
            The data object received.

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
    ) except *:
        """
        Register the strategy with a trader.

        Parameters
        ----------
        trader_id : TraderId
            The trader_id for the strategy.
        clock : Clock
            The clock for the strategy.
        logger : Logger
            The logger for the strategy.

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
        self.data = engine.cache

    cpdef void register_execution_engine(self, ExecutionEngine engine) except *:
        """
        Register the strategy with the given execution engine.

        Parameters
        ----------
        engine : ExecutionEngine
            The execution engine to register.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(engine, "engine")

        self._exec_engine = engine
        self.execution = engine.cache
        self.portfolio = engine.portfolio

    cpdef void register_indicator_for_quote_ticks(self, Symbol symbol, Indicator indicator) except *:
        """
        Register the given indicator with the strategy to receive quote tick
        data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The symbol for tick updates.
        indicator : Indicator
            The indicator to register.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(indicator, "indicator")

        if indicator not in self._indicators:
            self._indicators.append(indicator)

        if symbol not in self._indicators_for_quotes:
            self._indicators_for_quotes[symbol] = []  # type: list[Indicator]

        if indicator not in self._indicators_for_quotes[symbol]:
            self._indicators_for_quotes[symbol].append(indicator)
            self.log.info(f"Indicator {indicator} registered for {symbol} quote ticks.")
        else:
            self.log.error(f"Indicator {indicator} already registered for {symbol} quote ticks.")

    cpdef void register_indicator_for_trade_ticks(self, Symbol symbol, Indicator indicator) except *:
        """
        Register the given indicator with the strategy to receive trade tick
        data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The symbol for tick updates.
        indicator : indicator
            The indicator to register.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(indicator, "indicator")

        if indicator not in self._indicators:
            self._indicators.append(indicator)

        if symbol not in self._indicators_for_trades:
            self._indicators_for_trades[symbol] = []  # type: list[Indicator]

        if indicator not in self._indicators_for_trades[symbol]:
            self._indicators_for_trades[symbol].append(indicator)
            self.log.info(f"Indicator {indicator} registered for {symbol} trade ticks.")
        else:
            self.log.error(f"Indicator {indicator} already registered for {symbol} trade ticks.")

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
            self.log.info(f"Indicator {indicator} registered for {bar_type} bars.")
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
        Exceptions raised in `on_save` will be caught, logged, and reraised.

        """
        self._check_trader_registered()

        self.log.info("Saving state...")

        cpdef dict state = {"OrderIdCount": self.order_factory.count_c()}

        try:
            user_state = self.on_save()
        except Exception as ex:
            self.log.exception(ex)
            raise ex  # Invalid state information could be saved

        self.log.info("Saved state.")

        return {**state, **user_state}

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
        Exceptions raised in `on_load` will be caught, logged, and reraised.

        """
        Condition.not_none(state, "state")

        self._check_trader_registered()

        self.log.debug("Loading state...")

        cdef int order_id_count = state.get("OrderIdCount", 0)
        self.order_factory.set_count(order_id_count)
        self.log.info(f"Setting OrderIdGenerator count to {order_id_count}.")

        try:
            self.on_load(state)
        except Exception as ex:
            self.log.exception(ex)
            raise ex

        self.log.info("Loaded state.")

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
        Condition.not_none(self._data_engine, "data_engine")

        cdef Subscribe subscribe = Subscribe(
            venue=symbol.venue,
            data_type=Instrument,
            metadata={SYMBOL: symbol},
            handler=self.handle_instrument,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        self._data_engine.execute(subscribe)

        self.log.info(f"Subscribed to {symbol} <Instrument> data.")

    cpdef void subscribe_quote_ticks(self, Symbol symbol) except *:
        """
        Subscribe to `QuoteTick` data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to subscribe to.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data_engine, "data_client")

        cdef Subscribe subscribe = Subscribe(
            venue=symbol.venue,
            data_type=QuoteTick,
            metadata={SYMBOL: symbol},
            handler=self.handle_quote_tick,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        self._data_engine.execute(subscribe)

        self.log.info(f"Subscribed to {symbol} <QuoteTick> data.")

    cpdef void subscribe_trade_ticks(self, Symbol symbol) except *:
        """
        Subscribe to `TradeTick` data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to subscribe to.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data_engine, "data_engine")

        cdef Subscribe subscribe = Subscribe(
            venue=symbol.venue,
            data_type=TradeTick,
            metadata={SYMBOL: symbol},
            handler=self.handle_trade_tick,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        self._data_engine.execute(subscribe)

        self.log.info(f"Subscribed to {symbol} <TradeTick> data.")

    cpdef void subscribe_bars(self, BarType bar_type) except *:
        """
        Subscribe to `Bar` data for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to subscribe to.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(self._data_engine, "data_client")

        cdef Subscribe subscribe = Subscribe(
            venue=bar_type.symbol.venue,
            data_type=Bar,
            metadata={BAR_TYPE: bar_type},
            handler=self.handle_bar,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        self._data_engine.execute(subscribe)

        self.log.info(f"Subscribed to {bar_type} <Bar> data.")

    cpdef void unsubscribe_instrument(self, Symbol symbol) except *:
        """
        Unsubscribe from `Instrument` data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The instrument symbol to unsubscribe from.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data_engine, "data_client")

        cdef Unsubscribe unsubscribe = Unsubscribe(
            venue=symbol.venue,
            data_type=Instrument,
            metadata={SYMBOL: symbol},
            handler=self.handle_instrument,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        self._data_engine.execute(unsubscribe)

        self.log.info(f"Unsubscribed from {symbol} <Instrument> data.")

    cpdef void unsubscribe_quote_ticks(self, Symbol symbol) except *:
        """
        Unsubscribe from `QuoteTick` data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to unsubscribe from.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data_engine, "data_client")

        cdef Unsubscribe unsubscribe = Unsubscribe(
            venue=symbol.venue,
            data_type=QuoteTick,
            metadata={SYMBOL: symbol},
            handler=self.handle_quote_tick,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        self._data_engine.execute(unsubscribe)

        self.log.info(f"Unsubscribed from {symbol} <QuoteTick> data.")

    cpdef void unsubscribe_trade_ticks(self, Symbol symbol) except *:
        """
        Unsubscribe from `TradeTick` data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to unsubscribe from.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data_engine, "data_engine")

        cdef Unsubscribe unsubscribe = Unsubscribe(
            venue=symbol.venue,
            data_type=TradeTick,
            metadata={SYMBOL: symbol},
            handler=self.handle_trade_tick,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        self._data_engine.execute(unsubscribe)

        self.log.info(f"Unsubscribed from {symbol} <TradeTick> data.")

    cpdef void unsubscribe_bars(self, BarType bar_type) except *:
        """
        Unsubscribe from `Bar` data for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to unsubscribe from.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(self._data_engine, "data_engine")

        cdef Unsubscribe unsubscribe = Unsubscribe(
            venue=bar_type.symbol.venue,
            data_type=Bar,
            metadata={BAR_TYPE: bar_type},
            handler=self.handle_bar,
            command_id=self.uuid_factory.generate(),
            command_timestamp=self.clock.utc_now(),
        )

        self._data_engine.execute(unsubscribe)

        self.log.info(f"Unsubscribed from {bar_type} <Bar> data.")

# -- REQUESTS --------------------------------------------------------------------------------------

    cpdef void request_quote_ticks(
        self,
        Symbol symbol,
        datetime from_datetime=None,
        datetime to_datetime=None,
    ) except *:
        """
        Request historical quote ticks for the given parameters.

        If datetimes are `None` then will request the most recent data.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol for the request.
        from_datetime : datetime, optional
            The specified from datetime for the data.
        to_datetime : datetime, optional
            The specified to datetime for the data. If None then will default
            to the current datetime.

        Notes
        -----
        Always limited to the tick capacity of the `DataEngine` cache.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data_engine, "data_engine")
        if from_datetime is not None and to_datetime is not None:
            Condition.true(from_datetime < to_datetime, "from_datetime < to_datetime")

        cdef DataRequest request = DataRequest(
            venue=symbol.venue,
            data_type=QuoteTick,
            metadata={
                SYMBOL: symbol,
                FROM_DATETIME: from_datetime,
                TO_DATETIME: to_datetime,
                LIMIT: self._data_engine.cache.tick_capacity,
            },
            callback=self.handle_quote_ticks,
            request_id=self.uuid_factory.generate(),
            request_timestamp=self.clock.utc_now(),
        )

        self._data_engine.send(request)

    cpdef void request_trade_ticks(
        self,
        Symbol symbol,
        datetime from_datetime=None,
        datetime to_datetime=None,
    ) except *:
        """
        Request historical trade ticks for the given parameters.

        If datetimes are `None` then will request the most recent data.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol for the request.
        from_datetime : datetime, optional
            The specified from datetime for the data.
        to_datetime : datetime, optional
            The specified to datetime for the data. If None then will default
            to the current datetime.

        Notes
        -----
        Always limited to the tick capacity of the `DataEngine` cache.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data_engine, "data_engine")
        if from_datetime is not None and to_datetime is not None:
            Condition.true(from_datetime < to_datetime, "from_datetime < to_datetime")

        cdef DataRequest request = DataRequest(
            venue=symbol.venue,
            data_type=TradeTick,
            metadata={
                SYMBOL: symbol,
                FROM_DATETIME: from_datetime,
                TO_DATETIME: to_datetime,
                LIMIT: self._data_engine.cache.tick_capacity,
            },
            callback=self.handle_trade_ticks,
            request_id=self.uuid_factory.generate(),
            request_timestamp=self.clock.utc_now(),
        )

        self._data_engine.send(request)

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
            Condition.true(from_datetime < to_datetime, "from_datetime < to_datetime")

        cdef DataRequest request = DataRequest(
            venue=bar_type.symbol.venue,
            data_type=Bar,
            metadata={
                BAR_TYPE: bar_type,
                FROM_DATETIME: from_datetime,
                TO_DATETIME: to_datetime,
                LIMIT: self._data_engine.cache.bar_capacity,
            },
            callback=self.handle_bars,
            request_id=self.uuid_factory.generate(),
            request_timestamp=self.clock.utc_now(),
        )

        self._data_engine.send(request)

# -- TRADING COMMANDS ------------------------------------------------------------------------------

    cpdef void submit_order(self, Order order, PositionId position_id=None) except *:
        """
        Submit the given order, optionally for the given position identifier.

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
        Condition.not_none(self._exec_engine, "self._exec_engine")

        cdef Position position
        if position_id is None:
            # Null object pattern
            position_id = PositionId.null_c()

        cdef AccountId account_id = self.execution.account_id(order.symbol.venue)
        if account_id is None:
            self.log.error(
                f"Cannot submit {order} "
                f"(no account registered for {order.symbol.venue})."
            )
            return  # Cannot send command

        cdef SubmitOrder command = SubmitOrder(
            order.symbol.venue,
            self.trader_id,
            account_id,
            self.id,
            position_id,
            order,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        self.log.info(f"{CMD}{SENT} {command}.")
        self._exec_engine.execute(command)

    cpdef void submit_bracket_order(self, BracketOrder bracket_order) except *:
        """
        Submit the given bracket order.

        A `SubmitBracketOrder` command with be created and sent to the
        `ExecutionEngine`.

        Parameters
        ----------
        bracket_order : BracketOrder
            The bracket order to submit.

        """
        Condition.not_none(bracket_order, "bracket_order")
        Condition.not_none(self.trader_id, "self.trader_id")
        Condition.not_none(self._exec_engine, "self._exec_engine")

        cdef AccountId account_id = self.execution.account_id(bracket_order.entry.symbol.venue)
        if account_id is None:
            self.log.error(
                f"Cannot submit {bracket_order} "
                f"(no account registered for {bracket_order.entry.symbol.venue})."
            )
            return  # Cannot send command

        cdef SubmitBracketOrder command = SubmitBracketOrder(
            bracket_order.entry.symbol.venue,
            self.trader_id,
            account_id,
            self.id,
            bracket_order,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        self.log.info(f"{CMD}{SENT} {command}.")
        self._exec_engine.execute(command)

    cpdef void modify_order(
            self,
            PassiveOrder order,
            Quantity new_quantity=None,
            Price new_price=None,
    ) except *:
        """
        Modify the given order with the given quantity and/or price.

        A `ModifyOrder` command is created and then sent to the
        `ExecutionEngine`. Either one or both values must differ from the
        original order for the command to be valid.

        Parameters
        ----------
        order : PassiveOrder
            The order to modify.
        new_quantity : Quantity, optional
            The new quantity for the given order.
        new_price : Price, optional
            The new price for the given order.

        """
        Condition.not_none(order, "order")
        Condition.not_none(self.trader_id, "self.trader_id")
        Condition.not_none(self._exec_engine, "self._exec_engine")

        cdef bint modifying = False  # Set validation flag (must become true)
        cdef Quantity quantity = order.quantity
        cdef Price price = order.price

        if new_quantity is not None and new_quantity != quantity:
            modifying = True
            quantity = new_quantity

        if new_price is not None and new_price != price:
            modifying = True
            price = new_price

        if not modifying:
            self.log.error(
                "Cannot create command ModifyOrder "
                "(both new_quantity and new_price were None)."
            )
            return

        if order.account_id is None:
            self.log.error(f"Cannot modify order (no account assigned to order yet), {order}.")
            return  # Cannot send command

        cdef ModifyOrder command = ModifyOrder(
            order.symbol.venue,
            self.trader_id,
            order.account_id,
            order.cl_ord_id,
            quantity,
            price,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        self.log.info(f"{CMD}{SENT} {command}.")
        self._exec_engine.execute(command)

    cpdef void cancel_order(self, Order order) except *:
        """
        Cancel the given order.

        A `CancelOrder` command will be created and then sent to the
        `ExecutionEngine`.

        Parameters
        ----------
        order : Order
            The order to cancel.

        """
        Condition.not_none(order, "order")
        Condition.not_none(self.trader_id, "self.trader_id")
        Condition.not_none(self._exec_engine, "self._exec_engine")

        if order.account_id is None:
            self.log.error(f"Cannot cancel order (no account assigned to order yet), {order}.")
            return  # Cannot send command

        cdef CancelOrder command = CancelOrder(
            order.symbol.venue,
            self.trader_id,
            order.account_id,
            order.cl_ord_id,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        self.log.info(f"{CMD}{SENT} {command}.")
        self._exec_engine.execute(command)

    cpdef void cancel_all_orders(self, Symbol symbol) except *:
        """
        Cancel all orders for this strategy for the given symbol.

        All working orders in turn will have a `CancelOrder` command created and
        then sent to the `ExecutionEngine`.

        Parameters
        ----------
        symbol : Symbol, optional
            The symbol for the orders to cancel.

        """
        Condition.not_none(self._exec_engine, "self._exec_engine")

        cdef list working_orders = self.execution.orders_working(symbol, self.id)

        if not working_orders:
            self.log.info("No working orders to cancel.")
            return

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
        Condition.not_none(self._exec_engine, "self._exec_engine")

        if position.is_closed_c():
            self.log.warning(
                f"Cannot flatten position "
                f"(the position is already closed), {position}."
            )
            return  # Invalid command

        # Create flattening order
        cdef MarketOrder order = self.order_factory.market(
            position.symbol,
            Order.flatten_side_c(position.side),
            position.quantity,
        )

        # Create command
        cdef SubmitOrder submit_order = SubmitOrder(
            position.symbol.venue,
            self.trader_id,
            position.account_id,
            self.id,
            position.id,
            order,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        self.log.info(f"{CMD}{SENT} {submit_order}.")
        self._exec_engine.execute(submit_order)

    cpdef void flatten_all_positions(self, Symbol symbol) except *:
        """
        Flatten all positions for the given symbol for this strategy.

        All open positions in turn will have a closing `MarketOrder` created and
        then sent to the `ExecutionEngine` via `SubmitOrder` commands.

        Parameters
        ----------
        symbol : Symbol, optional
            The symbol for the positions to flatten.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._exec_engine, "self._exec_engine")

        cdef list positions_open = self.execution.positions_open(symbol, self.id)

        if not positions_open:
            self.log.info("No open positions to flatten.")
            return

        self.log.info(f"Flattening {len(positions_open)} open position(s)...")

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
                raise ex

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
        cdef list indicators = self._indicators_for_quotes.get(tick.symbol)  # Could be None
        cdef Indicator indicator
        if indicators is not None:
            for indicator in indicators:
                indicator.handle_quote_tick(tick)

        if is_historical:
            return  # Don't pass to on_tick()

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_quote_tick(tick)
            except Exception as ex:
                self.log.exception(ex)
                raise ex

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
        cdef Symbol symbol = first.symbol if first is not None else None

        if length > 0:
            self.log.info(f"Received <QuoteTick[{length}]> data for {symbol}.")
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
        cdef list indicators = self._indicators_for_trades.get(tick.symbol)  # Could be None
        cdef Indicator indicator
        if indicators is not None:
            for indicator in indicators:
                indicator.handle_trade_tick(tick)

        if is_historical:
            return  # Don't pass to on_tick()

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_trade_tick(tick)
            except Exception as ex:
                self.log.exception(ex)
                raise ex

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
        cdef Symbol symbol = first.symbol if first is not None else None

        if length > 0:
            self.log.info(f"Received <TradeTick[{length}]> data for {symbol}.")
        else:
            self.log.warning("Received <TradeTick[]> data with no ticks.")

        for i in range(length):
            self.handle_trade_tick(ticks[i], is_historical=True)

    cpdef void handle_bar(self, BarType bar_type, Bar bar, bint is_historical=False) except *:
        """
        Handle the given bar data.

        Calls `on_bar` if `strategy.state` is `RUNNING`.

        Parameters
        ----------
        bar_type : BarType
            The received bar type.
        bar : Bar
            The bar received.
        is_historical : bool
            If bar is historical then it won't be passed to `on_bar`.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(bar, "bar")

        # Update indicators
        cdef list indicators = self._indicators_for_bars.get(bar_type)  # Could be None
        cdef Indicator indicator
        if indicators is not None:
            for indicator in indicators:
                indicator.handle_bar(bar)

        if is_historical:
            return  # Don't pass to on_bar()

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_bar(bar_type, bar)
            except Exception as ex:
                self.log.exception(ex)
                raise ex

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cpdef void handle_bars(self, BarType bar_type, list bars) except *:
        """
        Handle the given bar data by handling each bar individually.

        Parameters
        ----------
        bar_type : BarType
            The received bar type.
        bars : list[Bar]
            The bars to handle.

        Warnings
        --------
        System method (not intended to be called by user code).

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(bars, "bars")  # Can be empty

        cdef int length = len(bars)
        cdef Bar first = bars[0] if length > 0 else None
        cdef Bar last = bars[length - 1] if length > 0 else None

        self.log.info(f"Received <Bar[{length}]> data for {bar_type}.")

        if length > 0 and first.timestamp > last.timestamp:
            raise RuntimeError(f"Cannot handle <Bar[{length}]> data, incorrectly sorted")

        for i in range(length):
            self.handle_bar(bar_type, bars[i], is_historical=True)

    cpdef void handle_data(self, data) except *:
        """
        Handle the given data object.

        Calls `on_data` if `strategy.state` is `RUNNING`.

        Parameters
        ----------
        data : object
            The received data object.

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
                raise ex

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
        self.handle_event_c(event)

    cdef void handle_event_c(self, Event event) except *:
        Condition.not_none(event, "event")

        if isinstance(event, (OrderRejected, OrderCancelReject)):
            self.log.warning(f"{RECV}{EVT} {event}.")
        else:
            self.log.info(f"{RECV}{EVT} {event}.")

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_event(event)
            except Exception as ex:
                self.log.exception(ex)
                raise ex
