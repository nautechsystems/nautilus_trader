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

import cython

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.component cimport create_component_fsm
from nautilus_trader.common.factories cimport OrderFactory
from nautilus_trader.common.logging cimport CMD
from nautilus_trader.common.logging cimport EVT
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.logging cimport RECV
from nautilus_trader.common.logging cimport SENT
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.fsm cimport InvalidStateTrigger
from nautilus_trader.data.engine cimport DataEngine
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.indicators.base.indicator cimport Indicator
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.c_enums.component_state cimport ComponentState
from nautilus_trader.model.c_enums.component_state cimport component_state_from_string
from nautilus_trader.model.c_enums.component_state cimport component_state_to_string
from nautilus_trader.model.c_enums.component_trigger cimport ComponentTrigger
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
from nautilus_trader.model.order cimport flatten_side
from nautilus_trader.model.position cimport Position
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick


cdef class TradingStrategy:
    """
    The base class for all trading strategies.
    """

    def __init__(self, str order_id_tag not None):
        """
        Initialize a new instance of the TradingStrategy class.

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

        # Identifiers
        self.id = StrategyId(self.__class__.__name__, order_id_tag)
        self.trader_id = None     # Initialized when registered with a trader

        # Public components
        self.clock = None          # Initialized when registered with a trader
        self.uuid_factory = None   # Initialized when registered with a trader
        self.log = None            # Initialized when registered with a trader
        # self.data = None         # Initialized when registered with the data engine
        self.execution = None      # Initialized when registered with the execution engine
        self.portfolio = None      # Initialized when registered with the execution engine
        self.order_factory = None  # Initialized when registered with a trader

        # Private components
        self._data_engine = None   # Initialized when registered with the data engine
        self._exec_engine = None   # Initialized when registered with the execution engine
        self._fsm = create_component_fsm()

        # Indicators
        self._indicators = []              # type: [Indicator]
        self._indicators_for_quotes = {}   # type: {Symbol, [Indicator]}
        self._indicators_for_trades = {}   # type: {Symbol, [Indicator]}
        self._indicators_for_bars = {}     # type: {BarType, [Indicator]}

    cpdef ComponentState state(self):
        """
        Return the trading strategies state.

        """
        return component_state_from_string(self.state_as_string())

    cpdef str state_as_string(self):
        """
        Return the trading strategies state as a string.

        Returns
        -------
        str

        """
        return component_state_to_string(self._fsm.state)

    def __eq__(self, TradingStrategy other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        Parameters
        ----------
        other : object
            The other object to equate.

        Returns
        -------
        bool

        """
        return self.id == other.id

    def __ne__(self, TradingStrategy other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        Parameters
        ----------
        other : object
            The other object to equate.

        Returns
        -------
        bool

        """
        return self.id != other.id

    def __hash__(self) -> int:
        """
        Return the hash code of this object.

        Returns
        -------
        int

        """
        return hash(self.id.value)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return f"{self.__class__.__name__}({self.id.value})"

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        Returns
        -------
        str

        """
        return f"<{str(self)} object at {id(self)}>"

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

        Note: 'OrderIdCount' and 'PositionIdCount' are reserved keys for
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

# -- REGISTRATION METHODS --------------------------------------------------------------------------

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

        self.trader_id = trader_id
        self.clock = clock
        self.clock.register_default_handler(self.handle_event)
        self.uuid_factory = uuid_factory
        self.log = LoggerAdapter(self.id.value, logger)

        self.order_factory = OrderFactory(
            strategy_id=self.id,
            id_tag_trader=self.trader_id.tag,
            id_tag_strategy=self.id.tag,
            clock=self.clock,
            uuid_factory=self.uuid_factory,
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
            self._indicators_for_quotes[symbol] = []  # type: [Indicator]

        if indicator not in self._indicators_for_quotes[symbol]:
            self._indicators_for_quotes[symbol].append(indicator)
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
            self._indicators_for_trades[symbol] = []  # type: [Indicator]

        if indicator not in self._indicators_for_trades[symbol]:
            self._indicators_for_trades[symbol].append(indicator)
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
            self._indicators_for_bars[bar_type] = []  # type: [Indicator]

        if indicator not in self._indicators_for_bars[bar_type]:
            self._indicators_for_bars[bar_type].append(indicator)
        else:
            self.log.error(f"Indicator {indicator} already registered for {bar_type} bars.")

# -- HANDLER METHODS -------------------------------------------------------------------------------

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
                self.log.exception(ex)
                self.stop()  # Halt strategy

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cpdef void handle_quote_ticks(self, list ticks) except *:
        """
        Handle the given list of ticks by handling each tick individually.

        Parameters
        ----------
        ticks : List[QuoteTick]
            The received ticks.

        """
        Condition.not_none(ticks, "ticks")  # Could be empty

        cdef int length = len(ticks)
        cdef Symbol symbol = ticks[0].symbol if length > 0 else None

        if length > 0:
            self.log.info(f"Received <QuoteTick[{length}]> data for {symbol}.")
        else:
            self.log.warning("Received <QuoteTick[]> data with no ticks.")

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
                self.log.exception(ex)
                self.stop()  # Halt strategy

    @cython.boundscheck(False)
    @cython.wraparound(False)
    cpdef void handle_trade_ticks(self, list ticks) except *:
        """
        Handle the given list of ticks by handling each tick individually.

        Parameters
        ----------
        ticks : List[TradeTick]
            The received ticks.

        """
        Condition.not_none(ticks, "ticks")  # Could be empty

        cdef int length = len(ticks)
        cdef Symbol symbol = ticks[0].symbol if length > 0 else None

        if length > 0:
            self.log.info(f"Received <TradeTick[{length}]> data for {symbol}.")
        else:
            self.log.warning("Received <TradeTick[]> data with no ticks.")

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
                self.log.exception(ex)
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
        bars : List[Bar]
            The bars to handle.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(bars, "bars")  # Can be empty

        cdef int length = len(bars)

        self.log.info(f"Received <Bar[{length}]> data for {bar_type}.")

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
                self.log.exception(ex)
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
            self.log.warning(f"{RECV}{EVT} {event}.")
        else:
            self.log.info(f"{RECV}{EVT} {event}.")

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_event(event)
            except Exception as ex:
                self.log.exception(ex)
                self.stop()  # Halt strategy

# -- DATA METHODS ----------------------------------------------------------------------------------

    cpdef void request_quote_ticks(self, Symbol symbol) except *:
        """
        Request the historical quote ticks for the given parameters from the data service.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol for the request.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data_engine, "data_client")

        self._data_engine.request_quote_ticks(
            symbol=symbol,
            from_datetime=None,
            to_datetime=None,
            limit=self._data_engine.tick_capacity,
            callback=self.handle_quote_ticks,
        )

    cpdef void request_trade_ticks(self, Symbol symbol) except *:
        """
        Request the historical trade ticks for the given parameters from the data service.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol for the request.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data_engine, "data_client")

        self._data_engine.request_trade_ticks(
            symbol=symbol,
            from_datetime=None,
            to_datetime=None,
            limit=self._data_engine.tick_capacity,
            callback=self.handle_trade_ticks)

    cpdef void request_bars(self, BarType bar_type) except *:
        """
        Request the historical bars for the given parameters from the data service.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the request.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(self._data_engine, "data_client")

        self._data_engine.request_bars(
            bar_type=bar_type,
            from_datetime=None,
            to_datetime=None,
            limit=self._data_engine.bar_capacity,
            callback=self.handle_bars)

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

        self._data_engine.subscribe_quote_ticks(symbol, self.handle_quote_tick)
        self.log.info(f"Subscribed to {symbol} <QuoteTick> data.")

    cpdef void subscribe_trade_ticks(self, Symbol symbol) except *:
        """
        Subscribe to <TradeTick> data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to subscribe to.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data_engine, "data_client")

        self._data_engine.subscribe_trade_ticks(symbol, self.handle_trade_tick)
        self.log.info(f"Subscribed to {symbol} <TradeTick> data.")

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

        self._data_engine.subscribe_bars(bar_type, self.handle_bar)
        self.log.info(f"Subscribed to {bar_type} <Bar> data.")

    cpdef void subscribe_instrument(self, Symbol symbol) except *:
        """
        Subscribe to <Instrument> data for the given symbol.

        Parameters
        ----------
        symbol : Instrument
            The instrument symbol to subscribe to.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data_engine, "data_client")

        self._data_engine.subscribe_instrument(symbol, self.handle_data)
        self.log.info(f"Subscribed to {symbol} <Instrument> data.")

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

        self._data_engine.unsubscribe_quote_ticks(symbol, self.handle_quote_tick)
        self.log.info(f"Unsubscribed from {symbol} <QuoteTick> data.")

    cpdef void unsubscribe_trade_ticks(self, Symbol symbol) except *:
        """
        Unsubscribe from <TradeTick> data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to unsubscribe from.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data_engine, "data_client")

        self._data_engine.unsubscribe_trade_ticks(symbol, self.handle_trade_tick)
        self.log.info(f"Unsubscribed from {symbol} <TradeTick> data.")

    cpdef void unsubscribe_bars(self, BarType bar_type) except *:
        """
        Unsubscribe from <Bar> data for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to unsubscribe from.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(self._data_engine, "data_client")

        self._data_engine.unsubscribe_bars(bar_type, self.handle_bar)
        self.log.info(f"Unsubscribed from {bar_type} <Bar> data.")

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

        self._data_engine.unsubscribe_instrument(symbol, self.handle_data)
        self.log.info(f"Unsubscribed from {symbol} <Instrument> data.")

    cpdef list symbols(self):
        """
        Return a list of all instrument symbols held by the data engine.

        Returns
        -------
        List[Instrument]

        """
        Condition.not_none(self._data_engine, "data_client")

        return self._data_engine.symbols()

    cpdef list instruments(self):
        """
        Return a dictionary of all instruments for the given venue (if any).

        Returns
        -------
        Dict[Symbol, Instrument]

        """
        Condition.not_none(self._data_engine, "data_client")

        return self._data_engine.instruments()

    cpdef list quote_ticks(self, Symbol symbol):
        """
        Return the quote ticks for the given symbol (returns a copy of the internal deque).

        Parameters
        ----------
        symbol : Symbol
            The symbol for the ticks to get.

        Returns
        -------
        List[QuoteTick]

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data_engine, "data client")

        return self._data_engine.quote_ticks(symbol)

    cpdef list trade_ticks(self, Symbol symbol):
        """
        Return the trade ticks for the given symbol (returns a copy of the internal deque).

        Parameters
        ----------
        symbol : Symbol
            The symbol for the ticks to get.

        Returns
        -------
        List[TradeTick]

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data_engine, "data client")

        return self._data_engine.trade_ticks(symbol)

    cpdef list bars(self, BarType bar_type):
        """
        Return the bars for the given bar type (returns a copy of the internal deque).

        Parameters
        ----------
        bar_type : BarType
            The bar type to get.

        Returns
        -------
        List[Bar]

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(self._data_engine, "data client")

        return self._data_engine.bars(bar_type)

    cpdef Instrument instrument(self, Symbol symbol):
        """
        Return the instrument corresponding to the given symbol (if found).

        Parameters
        ----------
        symbol : Symbol
            The symbol of the instrument to return.

        Returns
        -------
        Instrument or None

        """
        Condition.not_none(self._data_engine, "data_client")

        return self._data_engine.instrument(symbol)

    cpdef QuoteTick quote_tick(self, Symbol symbol, int index=0):
        """
        Return the quote tick for the given symbol at the given index or last if no index specified.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the tick to get.
        index : int
            The optional index for the tick to get.

        Returns
        -------
        QuoteTick

        Raises
        ------
        IndexError
            If tick index is out of range.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data_engine, "data client")

        return self._data_engine.quote_tick(symbol, index)

    cpdef TradeTick trade_tick(self, Symbol symbol, int index=0):
        """
        Return the trade tick for the given symbol at the given index or last if no index specified.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the tick to get.
        index : int, optional
            The index for the tick to get.

        Returns
        -------
        TradeTick

        Raises
        ------
        IndexError
            If tick index is out of range.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data_engine, "data client")

        return self._data_engine.trade_tick(symbol, index)

    cpdef Bar bar(self, BarType bar_type, int index=0):
        """
        Return the bar for the given bar type at the given index or last if no index specified.

        Parameters
        ----------
        bar_type : BarType
            The bar type to get.
        index : int, optional
            The index for the bar to get.

        Returns
        -------
        Bar

        Raises
        ------
        IndexError
            If the bar index is out of range.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(self._data_engine, "data client")

        return self._data_engine.bar(bar_type, index)

    cpdef int quote_tick_count(self, Symbol symbol):
        """
        Return the count of quote ticks held by the strategy for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the ticks.

        Returns
        -------
        int

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data_engine, "data client")

        return self._data_engine.quote_tick_count(symbol)

    cpdef int trade_tick_count(self, Symbol symbol):
        """
        Return the count of trade ticks held by the strategy for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the ticks.

        Returns
        -------
        int

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data_engine, "data client")

        return self._data_engine.trade_tick_count(symbol)

    cpdef int bar_count(self, BarType bar_type):
        """
        Return the count of bars held by the strategy for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to count.

        Returns
        -------
        int

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(self._data_engine, "data client")

        return self._data_engine.bar_count(bar_type)

    cpdef bint has_quote_ticks(self, Symbol symbol) except *:
        """
        Return a value indicating whether the strategy has quote ticks for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the ticks.

        Returns
        -------
        bool

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data_engine, "data client")

        return self._data_engine.has_quote_ticks(symbol)

    cpdef bint has_trade_ticks(self, Symbol symbol) except *:
        """
        Return a value indicating whether the strategy has trade ticks for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the ticks.

        Returns
        -------
        bool

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data_engine, "data client")

        return self._data_engine.has_trade_ticks(symbol)

    cpdef bint has_bars(self, BarType bar_type) except *:
        """
        Return a value indicating whether the strategy has bars for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the bars.

        Returns
        -------
        bool

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_none(self._data_engine, "data client")

        return self._data_engine.has_bars(bar_type)

# -- INDICATOR METHODS -----------------------------------------------------------------------------

    cpdef list registered_indicators(self):
        """
        Return the registered indicators for the strategy (returns copy of internal list).

        Returns
        -------
        List[Indicator]

        """
        return self._indicators.copy()

    cpdef bint indicators_initialized(self) except *:
        """
        Return a value indicating whether all indicators are initialized.

        Returns
        -------
        bool

        """
        cdef Indicator indicator
        for indicator in self._indicators:
            if indicator.initialized is False:
                return False
        return True

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void start(self) except *:
        """
        Start the trading strategy.

        Calls on_start().
        """
        try:
            self._fsm.trigger(ComponentTrigger.START)
        except InvalidStateTrigger as ex:
            self.log.exception(ex)
            self.stop()  # Do not start strategy in an invalid state
            return

        self.log.info(f"state={self._fsm.state_as_string()}...")

        if self._data_engine is None:
            self.log.error("Cannot start strategy (the data engine is not registered).")
            return

        if self._exec_engine is None:
            self.log.error("Cannot start strategy (the execution engine is not registered).")
            return

        try:
            self.on_start()
        except Exception as ex:
            self.log.exception(ex)
            self.stop()
            return

        self._fsm.trigger(ComponentTrigger.RUNNING)
        self.log.info(f"state={self._fsm.state_as_string()}.")

    cpdef void stop(self) except *:
        """
        Stop the trading strategy.

        Calls on_stop().
        """
        try:
            self._fsm.trigger(ComponentTrigger.STOP)
        except InvalidStateTrigger as ex:
            self.log.exception(ex)
            return

        self.log.info(f"state={self._fsm.state_as_string()}...")

        # Clean up clock
        cdef list timer_names = self.clock.get_timer_names()
        self.clock.cancel_all_timers()

        cdef str name
        for name in timer_names:
            self.log.info(f"Cancelled Timer(name={name}).")

        try:
            self.on_stop()
        except Exception as ex:
            self.log.exception(ex)

        self._fsm.trigger(ComponentTrigger.STOPPED)
        self.log.info(f"state={self._fsm.state_as_string()}.")

    cpdef void resume(self) except *:
        """
        Resume the trading strategy.

        Calls on_resume().
        """
        try:
            self._fsm.trigger(ComponentTrigger.RESUME)
        except InvalidStateTrigger as ex:
            self.log.exception(ex)
            self.stop()  # Do not start strategy in an invalid state
            return

        self.log.info(f"state={self._fsm.state_as_string()}...")

        try:
            self.on_resume()
        except Exception as ex:
            self.log.exception(ex)
            self.stop()
            return

        self._fsm.trigger(ComponentTrigger.RUNNING)
        self.log.info(f"state={self._fsm.state_as_string()}.")

    cpdef void reset(self) except *:
        """
        Reset the trading strategy.

        Calls on_reset().
        All stateful values are reset to their initial value.
        """
        try:
            self._fsm.trigger(ComponentTrigger.RESET)
        except InvalidStateTrigger as ex:
            self.log.exception(ex)
            return

        self.log.info(f"state={self._fsm.state_as_string()}...")

        if self.order_factory is not None:
            self.order_factory.reset()

        self._indicators.clear()
        self._indicators_for_quotes.clear()
        self._indicators_for_trades.clear()
        self._indicators_for_bars.clear()

        try:
            self.on_reset()
        except Exception as ex:
            self.log.exception(ex)

        self._fsm.trigger(ComponentTrigger.RESET)  # State changes to initialized
        self.log.info(f"state={self._fsm.state_as_string()}.")

    cpdef void dispose(self) except *:
        """
        Dispose of the trading strategy.
        """
        try:
            self._fsm.trigger(ComponentTrigger.DISPOSE)
        except InvalidStateTrigger as ex:
            self.log.exception(ex)
            return

        self.log.info(f"state={self._fsm.state_as_string()}...")

        try:
            self.on_dispose()
        except Exception as ex:
            self.log.exception(ex)

        self._fsm.trigger(ComponentTrigger.DISPOSED)
        self.log.info(f"state={self._fsm.state_as_string()}.")

    cpdef dict save(self):
        """
        Return the strategy state dictionary to be saved.
        """
        cpdef dict state = {"OrderIdCount": self.order_factory.count()}

        try:
            user_state = self.on_save()
        except Exception as ex:
            self.log.exception(ex)

        return {**state, **user_state}

    cpdef void load(self, dict state) except *:
        """
        Load the strategy state from the give state dictionary.

        Parameters
        ----------
        state : Dict[str, object]
            The state dictionary.

        """
        Condition.not_empty(state, "state")

        order_id_count = state.get(b'OrderIdCount')
        if order_id_count:
            order_id_count = int(order_id_count.decode("utf8"))
            self.order_factory.set_count(order_id_count)
            self.log.info(f"Setting OrderIdGenerator count to {order_id_count}.")

        try:
            self.on_load(state)
        except Exception as ex:
            self.log.exception(ex)

    cpdef void submit_order(self, Order order, PositionId position_id=None) except *:
        """
        Send a submit order command with the given order to the execution engine.

        Parameters
        ----------
        order : Order
            The order to submit.
        position_id : PositionId, optional
            The position identifier to submit the order against.

        """
        Condition.not_none(order, "order")
        Condition.not_none(self.trader_id, "trader_id")
        Condition.not_none(self._exec_engine, "exec_engine")

        if position_id is None:
            # Null object pattern
            position_id = PositionId.null()

        cdef AccountId account_id = self._exec_engine.cache.account_for_venue(order.symbol.venue)
        if account_id is None:
            self.log.error(f"Cannot submit {order} "
                           f"(no account registered for {order.symbol.venue}).")
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

    cpdef void submit_bracket_order(self, BracketOrder bracket_order, bint register=True) except *:
        """
        Send a SubmitBracketOrder command with the given bracket order to the
        execution engine.

        Parameters
        ----------
        bracket_order : BracketOrder
            The bracket order to submit.
        register : bool, optional
            If the stop-loss and take-profit orders should be registered as such.

        """
        Condition.not_none(bracket_order, "bracket_order")
        Condition.not_none(self.trader_id, "trader_id")
        Condition.not_none(self._exec_engine, "_exec_engine")

        cdef AccountId account_id = self._exec_engine.cache.account_for_venue(bracket_order.entry.symbol.venue)
        if account_id is None:
            self.log.error(f"Cannot submit {bracket_order}"
                           f"(no account registered for {bracket_order.entry.symbol.venue}).")
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
            Order order,
            Quantity new_quantity=None,
            Price new_price=None,
    ) except *:
        """
        Send a ModifyOrder command with the given new_quantity, and new_price to
        the execution engine.

        Either one or both values must differ from the original order to be
        valid.

        Parameters
        ----------
        order : Order
            The order to modify.
        new_quantity : Quantity, optional
            The new quantity for the given order.
        new_price : Price, optional
            The new price for the given order.

        """
        Condition.not_none(order, "order")
        Condition.not_none(self.trader_id, "trader_id")
        Condition.not_none(self._exec_engine, "_exec_engine")

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
            self.log.error("Cannot send command ModifyOrder "
                           "(both new_quantity and new_price were None).")
            return

        cdef AccountId account_id = self._exec_engine.cache.account_for_venue(order.symbol.venue)
        if account_id is None:
            self.log.error(f"Cannot modify {order} "
                           f"(no account registered for {order.symbol.venue}).")
            return  # Cannot send command

        cdef ModifyOrder command = ModifyOrder(
            order.symbol.venue,
            self.trader_id,
            account_id,
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
        Send a CancelOrder command for the given order to the execution engine.

        Parameters
        ----------
        order : Order
            The order to cancel.

        """
        Condition.not_none(order, "order")
        Condition.not_none(self.trader_id, "trader_id")
        Condition.not_none(self._exec_engine, "_exec_engine")

        cdef AccountId account_id = self._exec_engine.cache.account_for_venue(order.symbol.venue)
        if account_id is None:
            self.log.error(f"Cannot cancel {order} "
                           f"(no account registered for {order.symbol.venue}).")
            return  # Cannot send command

        cdef CancelOrder command = CancelOrder(
            order.symbol.venue,
            self.trader_id,
            account_id,
            order.cl_ord_id,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        self.log.info(f"{CMD}{SENT} {command}.")
        self._exec_engine.execute(command)

    cpdef void cancel_all_orders(self, Symbol symbol) except *:
        """
        Send a CancelAllOrders command for this strategy to the execution engine.
        """
        Condition.not_none(self._exec_engine, "_exec_engine")

        cdef list working_orders = self.execution.orders_working(symbol, self.id)

        if not working_orders:
            self.log.info("No working orders to cancel.")
            return

        cdef Order order
        cdef CancelOrder command
        for order in working_orders:
            self.cancel_order(order)

    cpdef void flatten_position(self, Position position) except *:
        """
        Send a FlattenPosition command for the given position identifier to the
        execution engine.

        Parameters
        ----------
        position : Position
            The position to flatten.

        """
        Condition.not_none(position, "position")
        Condition.not_none(self._exec_engine, "_exec_engine")

        if position.is_closed():
            self.log.warning(f"Cannot flatten {position} "
                             f"(the position is already closed).")
            return  # Invalid command

        cdef AccountId account_id = self._exec_engine.cache.account_for_venue(position.symbol.venue)
        if account_id is None:
            self.log.error(f"Cannot flatten {position} "
                           f"(no account registered for {position.symbol.venue}).")
            return  # Cannot send command

        # Create flattening order
        cdef MarketOrder order = self.order_factory.market(
            position.symbol,
            flatten_side(position.side),
            position.quantity,
        )

        # Create command
        cdef SubmitOrder submit_order = SubmitOrder(
            position.symbol.venue,
            self.trader_id,
            account_id,
            self.id,
            position.id,
            order,
            self.uuid_factory.generate(),
            self.clock.utc_now())

        self.log.info(f"{CMD}{SENT} {submit_order}.")
        self._exec_engine.execute(submit_order)

    cpdef void flatten_all_positions(self, Symbol symbol) except *:
        """
        Flatten all positions for the given symbol for this strategy.

        Parameters
        ----------
        symbol : Symbol, optional
            The specific symbol for the positions to flatten.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._exec_engine, "_exec_engine")

        cdef list positions_open = self.execution.positions_open(symbol, self.id)

        if not positions_open:
            self.log.info("No open positions to flatten.")
            return

        self.log.info(f"Flattening {len(positions_open)} open position(s)...")

        cdef Position position
        for position in positions_open:
            self.flatten_position(position)
