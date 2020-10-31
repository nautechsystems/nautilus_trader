# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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
import cython

from nautilus_trader.common.c_enums.component_state cimport ComponentState
from nautilus_trader.common.c_enums.component_state cimport component_state_to_string
from nautilus_trader.common.c_enums.component_trigger cimport ComponentTrigger
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.commands cimport RequestData
from nautilus_trader.common.commands cimport Subscribe
from nautilus_trader.common.commands cimport Unsubscribe
from nautilus_trader.common.component cimport create_component_fsm
from nautilus_trader.common.factories cimport OrderFactory
from nautilus_trader.common.logging cimport CMD
from nautilus_trader.common.logging cimport EVT
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.logging cimport RECV
from nautilus_trader.common.logging cimport SENT
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.constants cimport *  # str constants
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.fsm cimport InvalidStateTrigger
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


# noinspection: Object has warned attribute
# noinspection PyUnresolvedReferences
cdef class TradingStrategy:
    """
    The base class for all trading strategies.
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

        # Core components
        self._clock = None          # Initialized when registered with a trader
        self._uuid_factory = None   # Initialized when registered with a trader
        self._log = None            # Initialized when registered with a trader
        self._fsm = create_component_fsm()

        self._id = StrategyId(type(self).__name__, order_id_tag)
        self._trader_id = None      # Initialized when registered with a trader
        self._data_engine = None    # Initialized when registered with the data engine
        self._data = None           # Initialized when registered with the data engine
        self._exec_engine = None    # Initialized when registered with the execution engine
        self._execution = None      # Initialized when registered with the execution engine
        self._portfolio = None      # Initialized when registered with the execution engine
        self._order_factory = None  # Initialized when registered with a trader

        # Indicators
        self._indicators = []              # type: [Indicator]
        self._indicators_for_quotes = {}   # type: {Symbol, [Indicator]}
        self._indicators_for_trades = {}   # type: {Symbol, [Indicator]}
        self._indicators_for_bars = {}     # type: {BarType, [Indicator]}

    def __eq__(self, TradingStrategy other) -> bool:
        return self._id == other.id

    def __ne__(self, TradingStrategy other) -> bool:
        return self._id != other.id

    def __hash__(self) -> int:
        return hash(self._id.value)

    def __repr__(self) -> str:
        return f"{type(self).__name__}(id={self._id.value})"

    @property
    def id(self):
        """
        The trading strategies identifier.

        Returns
        -------
        StrategyId

        """
        return self._id

    @property
    def trader_id(self):
        """
        The trader identifier associated with the trading strategy.

        Returns
        -------
        TraderId

        Raises
        ------
        TypeError
            If the strategy has not been registered with a `Trader`.

        """
        Condition.not_none(self._trader_id, "trader_id")

        return self._trader_id

    @property
    def clock(self):
        """
        The trading strategies clock.

        Returns
        -------
        Clock

        Raises
        ------
        TypeError
            If the strategy has not been registered with a `Trader`.

        """
        Condition.not_none(self._clock, "clock")

        return self._clock

    @property
    def uuid_factory(self):
        """
        The trading strategies UUID factory.

        Returns
        -------
        UUIDFactory

        Raises
        ------
        TypeError
            If the strategy has not been registered with a `Trader`.

        """
        Condition.not_none(self._uuid_factory, "uuid_factory")

        return self._uuid_factory

    @property
    def log(self):
        """
        The trading strategies logger adapter.

        Returns
        -------
        LoggerAdapter

        Raises
        ------
        TypeError
            If the strategy has not been registered with a `Trader`.

        """
        Condition.not_none(self._log, "log")

        return self._log

    @property
    def data(self):
        """
        The read-only cache of the `DataEngine` the strategy is registered
        with.

        Returns
        -------
        DataCacheFacade

        Raises
        ------
        TypeError
            If the strategy has not been registered with a `DataEngine`.

        """
        Condition.not_none(self._data, "data")

        return self._data

    @property
    def execution(self):
        """
        The read-only cache of the `ExecutionEngine` the strategy is registered
        with.

        Returns
        -------
        ExecutionCacheFacade or None

        Raises
        ------
        TypeError
            If the strategy has not been registered with an `ExecutionEngine`.

        """
        Condition.not_none(self._execution, "execution")

        return self._execution

    @property
    def portfolio(self):
        """
        The read-only portfolio the trading strategy is registered with.

        Returns
        -------
        PortfolioFacade or None

        Raises
        ------
        TypeError
            If the strategy has not been registered with an `ExecutionEngine`.

        """
        Condition.not_none(self._portfolio, "portfolio")

        return self._portfolio

    @property
    def order_factory(self):
        """
        The trading strategies order factory.

        Returns
        -------
        OrderFactory or None
            If the strategy has not been registered with a `Trader`,
            then will return None.

        Raises
        ------
        TypeError
            If the strategy has not been registered with a `Trader`.

        """
        Condition.not_none(self._order_factory, "order_factory")

        return self._order_factory

    @property
    def registered_indicators(self):
        """
        Return the registered indicators for the strategy.

        Returns
        -------
        list[Indicator]

        """
        return self._indicators.copy()

    @property
    def indicators_initialized(self):
        """
        If all indicators are initialized.

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

    @property
    def state(self):
        """
        The trading strategies current state.

        Returns
        -------
        ComponentState

        """
        return self._fsm.state

    cdef str state_string(self):
        """
        Returns
        -------
        str
            The trading strategies current state as a string.

        """
        return component_state_to_string(self._fsm.state)

# -- ABSTRACT METHODS ------------------------------------------------------------------------------

    cpdef void on_start(self) except *:
        """
        Actions to be performed on strategy start.
        """
        pass  # Optionally override in subclass

    cpdef void on_quote_tick(self, QuoteTick tick) except *:
        """
        Actions to be performed when the strategy is running and receives a quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The tick received.

        """
        pass  # Optionally override in subclass

    cpdef void on_trade_tick(self, TradeTick tick) except *:
        """
        Actions to be performed when the strategy is running and receives a trade tick.

        Parameters
        ----------
        tick : TradeTick
            The tick received.

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

        """
        pass  # Optionally override in subclass

    cpdef void on_data(self, object data) except *:
        """
        Actions to be performed when the strategy is running and receives a data object.

        Parameters
        ----------
        data : object
            The data object received.

        """
        pass  # Optionally override in subclass

    cpdef void on_event(self, Event event) except *:
        """
        Actions to be performed when the strategy is running and receives an event.

        Parameters
        ----------
        event : Event
            The event received.

        """
        pass  # Optionally override in subclass

    cpdef void on_stop(self) except *:
        """
        Actions to be performed when the strategy is stopped.
        """
        pass  # Optionally override in subclass

    cpdef void on_resume(self) except *:
        """
        Actions to be performed when the strategy is stopped.
        """
        pass  # Optionally override in subclass

    cpdef void on_reset(self) except *:
        """
        Actions to be performed when the strategy is reset.
        """
        pass  # Optionally override in subclass

    cpdef dict on_save(self):
        """
        Actions to be performed when the strategy is saved.

        Create and return a state dictionary of values to be saved.

        Notes
        -----
        'OrderIdCount' and 'PositionIdCount' are reserved keys for
        the returned state dictionary.

        """
        return {}  # Optionally override in subclass

    cpdef void on_load(self, dict state) except *:
        """
        Actions to be performed when the strategy is loaded.

        Saved state values will be contained in the give state dictionary.
        """
        pass  # Optionally override in subclass

    cpdef void on_dispose(self) except *:
        """
        Actions to be performed when the strategy is disposed.

        Cleanup any resources used by the strategy here.
        """
        pass  # Optionally override in subclass

# -- REGISTRATION ----------------------------------------------------------------------------------

    cpdef void register_trader(
            self,
            TraderId trader_id,
            Clock clock,
            UUIDFactory uuid_factory,
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
        uuid_factory : UUIDFactory
            The uuid_factory for the strategy.
        logger : Logger
            The logger for the strategy.

        """
        Condition.not_none(trader_id, "trader_id")
        Condition.not_none(clock, "clock")
        Condition.not_none(uuid_factory, "uuid_factory")
        Condition.not_none(logger, "logger")

        self._trader_id = trader_id
        self._clock = clock
        self._clock.register_default_handler(self.handle_event)
        self._uuid_factory = uuid_factory
        self._log = LoggerAdapter(self._id.value, logger)

        self._order_factory = OrderFactory(
            trader_id=self._trader_id,
            strategy_id=self._id,
            clock=self._clock,
            uuid_factory=self._uuid_factory,
        )

    cpdef void register_data_engine(self, DataEngine engine) except *:
        """
        Register the strategy with the given data engine.

        Parameters
        ----------
        engine : DataEngine
            The data engine to register.

        """
        Condition.not_none(engine, "engine")

        self._data_engine = engine
        self._data = engine.cache

    cpdef void register_execution_engine(self, ExecutionEngine engine) except *:
        """
        Register the strategy with the given execution engine.

        Parameters
        ----------
        engine : ExecutionEngine
            The execution engine to register.

        """
        Condition.not_none(engine, "engine")

        self._exec_engine = engine
        self._execution = engine.cache
        self._portfolio = engine.portfolio

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
            self._indicators_for_quotes[symbol] = []  # type: [Indicator]

        if indicator not in self._indicators_for_quotes[symbol]:
            self._indicators_for_quotes[symbol].append(indicator)
        else:
            self._log.error(f"Indicator {indicator} already registered for {symbol} quote ticks.")

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
            self._indicators_for_trades[symbol] = []  # type: [Indicator]

        if indicator not in self._indicators_for_trades[symbol]:
            self._indicators_for_trades[symbol].append(indicator)
        else:
            self._log.error(f"Indicator {indicator} already registered for {symbol} trade ticks.")

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
            self._indicators_for_bars[bar_type] = []  # type: [Indicator]

        if indicator not in self._indicators_for_bars[bar_type]:
            self._indicators_for_bars[bar_type].append(indicator)
        else:
            self._log.error(f"Indicator {indicator} already registered for {bar_type} bars.")

# -- HANDLERS --------------------------------------------------------------------------------------

    cpdef void handle_quote_tick(self, QuoteTick tick, bint is_historical=False) except *:
        """"
        Handle the given tick.

        Parameters
        ----------
        tick : QuoteTick
            The received tick.
        is_historical : bool
            If tick is historical then it won't be passed to on_quote_tick().

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
                self._log.exception(ex)
                self.stop()  # Halt strategy

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cpdef void handle_quote_ticks(self, list ticks) except *:
        """
        Handle the given list of ticks by handling each tick individually.

        Parameters
        ----------
        ticks : list[QuoteTick]
            The received ticks.

        """
        Condition.not_none(ticks, "ticks")  # Could be empty

        cdef int length = len(ticks)
        cdef Symbol symbol = ticks[0].symbol if length > 0 else None

        if length > 0:
            self._log.info(f"Received <QuoteTick[{length}]> data for {symbol}.")
        else:
            self._log.warning("Received <QuoteTick[]> data with no ticks.")

        cdef int i
        for i in range(length):
            self.handle_quote_tick(ticks[i], is_historical=True)

    cpdef void handle_trade_tick(self, TradeTick tick, bint is_historical=False) except *:
        """"
        Handle the given tick.

        Parameters
        ----------
        tick : TradeTick
            The received trade tick.
        is_historical : bool
            If tick is historical then it won't be passed to on_trade_tick().

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
                self._log.exception(ex)
                self.stop()  # Halt strategy

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cpdef void handle_trade_ticks(self, list ticks) except *:
        """
        Handle the given list of ticks by handling each tick individually.

        Parameters
        ----------
        ticks : list[TradeTick]
            The received ticks.

        """
        Condition.not_none(ticks, "ticks")  # Could be empty

        cdef int length = len(ticks)
        cdef Symbol symbol = ticks[0].symbol if length > 0 else None

        if length > 0:
            self._log.info(f"Received <TradeTick[{length}]> data for {symbol}.")
        else:
            self._log.warning("Received <TradeTick[]> data with no ticks.")

        cdef int i
        for i in range(length):
            self.handle_trade_tick(ticks[i], is_historical=True)

    cpdef void handle_bar(self, BarType bar_type, Bar bar, bint is_historical=False) except *:
        """"
        Handle the given bar type and bar.

        Parameters
        ----------
        bar_type : BarType
            The received bar type.
        bar : Bar
            The bar received.
        is_historical : bool
            If bar is historical then it won't be passed to on_bar().

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
                self._log.exception(ex)
                self.stop()  # Halt strategy

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cpdef void handle_bars(self, BarType bar_type, list bars) except *:
        """
        Handle the given bar type and bars by handling each bar individually.

        Parameters
        ----------
        bar_type : BarType
            The received bar type.
        bars : list[Bar]
            The bars to handle.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(bars, "bars")  # Can be empty

        cdef int length = len(bars)

        self._log.info(f"Received <Bar[{length}]> data for {bar_type}.")

        if length > 0 and bars[0].timestamp > bars[length - 1].timestamp:
            raise RuntimeError("Cannot handle <Bar[]> data (incorrectly sorted).")

        cdef int i
        for i in range(length):
            self.handle_bar(bar_type, bars[i], is_historical=True)

    cpdef void handle_data(self, object data) except *:
        """
        Handle the given data object.

        Parameters
        ----------
        data : object
            The received data object.

        """
        Condition.not_none(data, "data")

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_data(data)
            except Exception as ex:
                self._log.exception(ex)
                self.stop()  # Halt strategy

    cpdef void handle_event(self, Event event) except *:
        """
        Hand the given event.

        Parameters
        ----------
        event : Event
            The received event.

        """
        Condition.not_none(event, "event")

        if isinstance(event, (OrderRejected, OrderCancelReject)):
            self._log.warning(f"{RECV}{EVT} {event}.")
        else:
            self._log.info(f"{RECV}{EVT} {event}.")

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_event(event)
            except Exception as ex:
                self._log.exception(ex)
                self.stop()  # Halt strategy

# -- STRATEGY COMMANDS -----------------------------------------------------------------------------

    cpdef void start(self) except *:
        """
        Start the trading strategy.

        Calls on_start().
        """
        try:
            self._fsm.trigger(ComponentTrigger.START)
        except InvalidStateTrigger as ex:
            self._log.exception(ex)
            self.stop()  # Do not start strategy in an invalid state
            return

        self._log.info(f"state={self._fsm.state_string()}...")

        if self._data_engine is None:
            self._log.error("Cannot start strategy (the data engine is not registered).")
            return

        if self._exec_engine is None:
            self._log.error("Cannot start strategy (the execution engine is not registered).")
            return

        try:
            self.on_start()
        except Exception as ex:
            self._log.exception(ex)
            self.stop()
            return

        self._fsm.trigger(ComponentTrigger.RUNNING)
        self._log.info(f"state={self._fsm.state_string()}.")

    cpdef void stop(self) except *:
        """
        Stop the trading strategy.

        Calls on_stop().
        """
        try:
            self._fsm.trigger(ComponentTrigger.STOP)
        except InvalidStateTrigger as ex:
            self._log.exception(ex)
            return

        self._log.info(f"state={self._fsm.state_string()}...")

        # Clean up clock
        cdef list timer_names = self._clock.timer_names
        self._clock.cancel_all_timers()

        cdef str name
        for name in timer_names:
            self._log.info(f"Cancelled Timer(name={name}).")

        try:
            self.on_stop()
        except Exception as ex:
            self._log.exception(ex)

        self._fsm.trigger(ComponentTrigger.STOPPED)
        self._log.info(f"state={self._fsm.state_string()}.")

    cpdef void resume(self) except *:
        """
        Resume the trading strategy.

        Calls on_resume().
        """
        try:
            self._fsm.trigger(ComponentTrigger.RESUME)
        except InvalidStateTrigger as ex:
            self._log.exception(ex)
            self.stop()  # Do not start strategy in an invalid state
            return

        self._log.info(f"state={self._fsm.state_string()}...")

        try:
            self.on_resume()
        except Exception as ex:
            self._log.exception(ex)
            self.stop()
            return

        self._fsm.trigger(ComponentTrigger.RUNNING)
        self._log.info(f"state={self._fsm.state_string()}.")

    cpdef void reset(self) except *:
        """
        Reset the trading strategy.

        All stateful values are reset to their initial value, then calls
        `on_reset()`.

        Raises
        ------
        InvalidStateTrigger
            If strategy state is RUNNING.

        """
        try:
            self._fsm.trigger(ComponentTrigger.RESET)
        except InvalidStateTrigger as ex:
            self._log.exception(ex)
            return

        self._log.info(f"state={self._fsm.state_string()}...")

        if self._order_factory:
            self._order_factory.reset()

        self._indicators.clear()
        self._indicators_for_quotes.clear()
        self._indicators_for_trades.clear()
        self._indicators_for_bars.clear()

        try:
            self.on_reset()
        except Exception as ex:
            self._log.exception(ex)

        self._fsm.trigger(ComponentTrigger.RESET)  # State changes to initialized
        self._log.info(f"state={self._fsm.state_string()}.")

    cpdef void dispose(self) except *:
        """
        Dispose of the trading strategy.
        """
        try:
            self._fsm.trigger(ComponentTrigger.DISPOSE)
        except InvalidStateTrigger as ex:
            self._log.exception(ex)
            return

        self._log.info(f"state={self._fsm.state_string()}...")

        try:
            self.on_dispose()
        except Exception as ex:
            self._log.exception(ex)

        self._fsm.trigger(ComponentTrigger.DISPOSED)
        self._log.info(f"state={self._fsm.state_string()}.")

    cpdef dict save(self):
        """
        Return the strategy state dictionary to be saved.
        """
        cpdef dict state = {"OrderIdCount": self._order_factory.count}

        try:
            user_state = self.on_save()
        except Exception as ex:
            self._log.exception(ex)

        return {**state, **user_state}

    cpdef void load(self, dict state) except *:
        """
        Load the strategy state from the give state dictionary.

        Parameters
        ----------
        state : dict[str, object]
            The state dictionary.

        """
        Condition.not_empty(state, "state")

        order_id_count = state.get(b'OrderIdCount')
        if order_id_count is not None:
            order_id_count = int(order_id_count.decode("utf8"))
            self._order_factory.set_count(order_id_count)
            self._log.info(f"Setting OrderIdGenerator count to {order_id_count}.")

        try:
            self.on_load(state)
        except Exception as ex:
            self._log.exception(ex)

# -- DATA COMMANDS ---------------------------------------------------------------------------------

    cpdef void request_quote_ticks(self, Symbol symbol) except *:
        """
        Request the historical quote ticks for the given parameters from the data service.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol for the request.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data_engine, "data_engine")

        cdef RequestData request = RequestData(
            data_type=QuoteTick,
            options={
                SYMBOL: symbol,
                FROM_DATETIME: None,
                TO_DATETIME: None,
                LIMIT: self._data_engine.cache.tick_capacity,
                CALLBACK: self.handle_quote_ticks,
            },
            command_id=self._uuid_factory.generate(),
            command_timestamp=self._clock.utc_now(),
        )

        self._data_engine.execute(request)

    cpdef void request_trade_ticks(self, Symbol symbol) except *:
        """
        Request the historical trade ticks for the given parameters from the data service.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol for the request.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data_engine, "data_engine")

        cdef RequestData request = RequestData(
            data_type=TradeTick,
            options={
                SYMBOL: symbol,
                FROM_DATETIME: None,
                TO_DATETIME: None,
                LIMIT: self._data_engine.cache.tick_capacity,
                CALLBACK: self.handle_trade_ticks,
            },
            command_id=self._uuid_factory.generate(),
            command_timestamp=self._clock.utc_now(),
        )

        self._data_engine.execute(request)

    cpdef void request_bars(self, BarType bar_type) except *:
        """
        Request the historical bars for the given parameters from the data service.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the request.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(self._data_engine, "data_engine")

        cdef RequestData request = RequestData(
            data_type=Bar,
            options={
                BAR_TYPE: bar_type,
                FROM_DATETIME: None,
                TO_DATETIME: None,
                LIMIT: self._data_engine.cache.bar_capacity,
                CALLBACK: self.handle_bars,
            },
            command_id=self._uuid_factory.generate(),
            command_timestamp=self._clock.utc_now(),
        )

        self._data_engine.execute(request)

    cpdef void subscribe_quote_ticks(self, Symbol symbol) except *:
        """
        Subscribe to <QuoteTick> data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to subscribe to.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data_engine, "data_client")

        cdef Subscribe subscribe = Subscribe(
            data_type=QuoteTick,
            options={
                SYMBOL: symbol,
                HANDLER: self.handle_quote_tick,
            },
            command_id=self._uuid_factory.generate(),
            command_timestamp=self._clock.utc_now(),
        )

        self._data_engine.execute(subscribe)

        self._log.info(f"Subscribed to {symbol} <QuoteTick> data.")

    cpdef void subscribe_trade_ticks(self, Symbol symbol) except *:
        """
        Subscribe to <TradeTick> data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to subscribe to.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data_engine, "data_engine")

        cdef Subscribe subscribe = Subscribe(
            data_type=TradeTick,
            options={
                SYMBOL: symbol,
                HANDLER: self.handle_trade_tick,
            },
            command_id=self._uuid_factory.generate(),
            command_timestamp=self._clock.utc_now(),
        )

        self._data_engine.execute(subscribe)

        self._log.info(f"Subscribed to {symbol} <TradeTick> data.")

    cpdef void subscribe_bars(self, BarType bar_type) except *:
        """
        Subscribe to <Bar> data for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to subscribe to.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(self._data_engine, "data_client")

        cdef Subscribe subscribe = Subscribe(
            data_type=Bar,
            options={
                BAR_TYPE: bar_type,
                HANDLER: self.handle_bar,
            },
            command_id=self._uuid_factory.generate(),
            command_timestamp=self._clock.utc_now(),
        )

        self._data_engine.execute(subscribe)

        self._log.info(f"Subscribed to {bar_type} <Bar> data.")

    cpdef void subscribe_instrument(self, Symbol symbol) except *:
        """
        Subscribe to <Instrument> data for the given symbol.

        Parameters
        ----------
        symbol : Instrument
            The instrument symbol to subscribe to.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data_engine, "data_engine")

        cdef Subscribe subscribe = Subscribe(
            data_type=Instrument,
            options={
                SYMBOL: symbol,
                HANDLER: self.handle_data,
            },
            command_id=self._uuid_factory.generate(),
            command_timestamp=self._clock.utc_now(),
        )

        self._data_engine.execute(subscribe)

        self._log.info(f"Subscribed to {symbol} <Instrument> data.")

    cpdef void unsubscribe_quote_ticks(self, Symbol symbol) except *:
        """
        Unsubscribe from <QuoteTick> data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to unsubscribe from.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data_engine, "data_client")

        cdef Unsubscribe unsubscribe = Unsubscribe(
            data_type=QuoteTick,
            options={
                SYMBOL: symbol,
                HANDLER: self.handle_quote_tick,
            },
            command_id=self._uuid_factory.generate(),
            command_timestamp=self._clock.utc_now(),
        )

        self._data_engine.execute(unsubscribe)

        self._log.info(f"Unsubscribed from {symbol} <QuoteTick> data.")

    cpdef void unsubscribe_trade_ticks(self, Symbol symbol) except *:
        """
        Unsubscribe from <TradeTick> data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to unsubscribe from.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data_engine, "data_engine")

        cdef Unsubscribe unsubscribe = Unsubscribe(
            data_type=TradeTick,
            options={
                SYMBOL: symbol,
                HANDLER: self.handle_trade_tick,
            },
            command_id=self._uuid_factory.generate(),
            command_timestamp=self._clock.utc_now(),
        )

        self._data_engine.execute(unsubscribe)

        self._log.info(f"Unsubscribed from {symbol} <TradeTick> data.")

    cpdef void unsubscribe_bars(self, BarType bar_type) except *:
        """
        Unsubscribe from <Bar> data for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to unsubscribe from.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(self._data_engine, "data_engine")

        cdef Unsubscribe unsubscribe = Unsubscribe(
            data_type=Bar,
            options={
                BAR_TYPE: bar_type,
                HANDLER: self.handle_bar,
            },
            command_id=self._uuid_factory.generate(),
            command_timestamp=self._clock.utc_now(),
        )

        self._data_engine.execute(unsubscribe)

        self._log.info(f"Unsubscribed from {bar_type} <Bar> data.")

    cpdef void unsubscribe_instrument(self, Symbol symbol) except *:
        """
        Unsubscribe from instrument data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The instrument symbol to unsubscribe from.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data_engine, "data_client")

        cdef Unsubscribe unsubscribe = Unsubscribe(
            data_type=Instrument,
            options={
                SYMBOL: symbol,
                HANDLER: self.handle_data,
            },
            command_id=self._uuid_factory.generate(),
            command_timestamp=self._clock.utc_now(),
        )

        self._data_engine.execute(unsubscribe)

        self._log.info(f"Unsubscribed from {symbol} <Instrument> data.")

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
        Condition.not_none(self._trader_id, "trader_id")
        Condition.not_none(self._exec_engine, "exec_engine")

        cdef Position position
        if position_id is None:
            # Null object pattern
            position_id = PositionId.null()

        cdef AccountId account_id = self._execution.account_id(order.symbol.venue)
        if account_id is None:
            self._log.error(
                f"Cannot submit {order} "
                f"(no account registered for {order.symbol.venue})."
            )
            return  # Cannot send command

        cdef SubmitOrder command = SubmitOrder(
            order.symbol.venue,
            self._trader_id,
            account_id,
            self._id,
            position_id,
            order,
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self._log.info(f"{CMD}{SENT} {command}.")
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
        Condition.not_none(self._trader_id, "trader_id")
        Condition.not_none(self._exec_engine, "exec_engine")

        cdef AccountId account_id = self._execution.account_id(bracket_order.entry.symbol.venue)
        if account_id is None:
            self._log.error(
                f"Cannot submit {bracket_order} "
                f"(no account registered for {bracket_order.entry.symbol.venue})."
            )
            return  # Cannot send command

        cdef SubmitBracketOrder command = SubmitBracketOrder(
            bracket_order.entry.symbol.venue,
            self._trader_id,
            account_id,
            self._id,
            bracket_order,
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self._log.info(f"{CMD}{SENT} {command}.")
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
        Condition.not_none(self._trader_id, "trader_id")
        Condition.not_none(self._exec_engine, "exec_engine")

        cdef bint modifying = False  # Set validation flag (must become true)
        cdef Quantity quantity = order.quantity
        cdef Price price = order.price

        if new_quantity is not None:
            modifying = True
            quantity = new_quantity

        if new_price is not None:
            modifying = True
            price = new_price

        if not modifying:
            self._log.error(
                "Cannot send command ModifyOrder "
                "(both new_quantity and new_price were None)."
            )
            return

        if order.account_id is None:
            self._log.error(f"Cannot modify {order} (no account assigned to order yet).")
            return  # Cannot send command

        cdef ModifyOrder command = ModifyOrder(
            order.symbol.venue,
            self._trader_id,
            order.account_id,
            order.cl_ord_id,
            quantity,
            price,
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self._log.info(f"{CMD}{SENT} {command}.")
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
        Condition.not_none(self._trader_id, "trader_id")
        Condition.not_none(self._exec_engine, "exec_engine")

        if order.account_id is None:
            self._log.error(f"Cannot cancel {order} (no account assigned to order yet).")
            return  # Cannot send command

        cdef CancelOrder command = CancelOrder(
            order.symbol.venue,
            self._trader_id,
            order.account_id,
            order.cl_ord_id,
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self._log.info(f"{CMD}{SENT} {command}.")
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
        Condition.not_none(self._exec_engine, "_exec_engine")

        cdef list working_orders = self._execution.orders_working(symbol, self._id)

        if not working_orders:
            self._log.info("No working orders to cancel.")
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
        Condition.not_none(self._exec_engine, "_exec_engine")

        if position.is_closed:
            self._log.warning(
                f"Cannot flatten {position} "
                f"(the position is already closed)."
            )
            return  # Invalid command

        # Create flattening order
        cdef MarketOrder order = self._order_factory.market(
            position.symbol,
            Order.flatten_side_c(position.side),
            position.quantity,
        )

        # Create command
        cdef SubmitOrder submit_order = SubmitOrder(
            position.symbol.venue,
            self._trader_id,
            position.account_id,
            self._id,
            position.id,
            order,
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self._log.info(f"{CMD}{SENT} {submit_order}.")
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
        Condition.not_none(self._exec_engine, "_exec_engine")

        cdef list positions_open = self._execution.positions_open(symbol, self._id)

        if not positions_open:
            self._log.info("No open positions to flatten.")
            return

        self._log.info(f"Flattening {len(positions_open)} open position(s)...")

        cdef Position position
        for position in positions_open:
            self.flatten_position(position)
