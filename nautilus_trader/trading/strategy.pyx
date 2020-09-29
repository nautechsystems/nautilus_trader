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
from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.commands cimport AccountInquiry
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport ModifyOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.events cimport Event
from nautilus_trader.model.events cimport OrderCancelReject
from nautilus_trader.model.events cimport OrderCancelled
from nautilus_trader.model.events cimport OrderDenied
from nautilus_trader.model.events cimport OrderExpired
from nautilus_trader.model.events cimport OrderFilled
from nautilus_trader.model.events cimport OrderInvalid
from nautilus_trader.model.events cimport OrderRejected
from nautilus_trader.model.events cimport PositionClosed
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.order cimport BracketOrder
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick


cdef tuple _ORDER_COMPLETION_TRIGGERS = (
    OrderInvalid,
    OrderDenied,
    OrderRejected,
    OrderCancelled,
    OrderExpired,
    OrderFilled,
)


cdef class TradingStrategy:
    """
    The base class for all trading strategies.
    """

    def __init__(
            self,
            str order_id_tag not None,
            bint flatten_on_stop=True,
            bint flatten_on_reject=True,
            bint cancel_all_orders_on_stop=True,
            bint reraise_exceptions=True,
    ):
        """
        Initialize a new instance of the TradingStrategy class.

        Parameters
        ----------
        order_id_tag : str
            The order_id tag for the strategy (must be unique at trader level).
        flatten_on_stop : bool
            If all strategy positions should be flattened on stop.
        flatten_on_reject : bool
            If open positions should be flattened on a child orders rejection.
        cancel_all_orders_on_stop : bool
            If all residual orders should be cancelled on stop.
        reraise_exceptions : bool
            If exceptions raised in handling methods should be re-raised.

        Raises
        ------
        ValueError
            If identifier_tag is not a valid string.

        """
        Condition.valid_string(order_id_tag, "identifier_tag")

        # Identification
        self.id = StrategyId(self.__class__.__name__, order_id_tag)
        self.trader_id = None     # Initialized when registered with a trader

        # Components
        self.clock = None         # Initialized when registered with a trader
        self.uuid_factory = None  # Initialized when registered with a trader
        self.log = None           # Initialized when registered with a trader

        # Management flags
        self.flatten_on_stop = flatten_on_stop
        self.flatten_on_reject = flatten_on_reject
        self.cancel_all_orders_on_stop = cancel_all_orders_on_stop
        self.reraise_exceptions = reraise_exceptions

        # Order / Position components
        self.order_factory = None          # Initialized when registered with a trader
        self._flattening_ids = set()       # type: {PositionId}
        self._stop_loss_ids = set()        # type: {ClientOrderId}
        self._take_profit_ids = set()      # type: {ClientOrderId}

        # Indicators
        self._indicators = []              # type: [Indicator]
        self._indicators_for_quotes = {}   # type: {Symbol, [Indicator]}
        self._indicators_for_trades = {}   # type: {Symbol, [Indicator]}
        self._indicators_for_bars = {}     # type: {BarType, [Indicator]}

        # Registerable modules
        self._data_engine = None  # Initialized when registered with the data engine
        self._exec_engine = None  # Initialized when registered with the execution engine

        self._fsm = create_component_fsm()

    cpdef bint equals(self, TradingStrategy other):
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
        return self.id.equals(other.id)

    cpdef ComponentState state(self):
        """
        Return the trading strategies state.
        """
        return self._fsm.state

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
        return self.equals(other)

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
        return not self.equals(other)

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
            id_tag_trader=self.trader_id.identifier_tag,
            id_tag_strategy=self.id.identifier_tag,
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

    cpdef void register_stop_loss(self, PassiveOrder order):
        """
        Register the given order to be managed as a stop-loss.

        If cancel_on_sl_reject management flag is set to True then associated
        position will be flattened if this order is rejected.

        Parameters
        ----------
        order : PassiveOrder
            The stop-loss order to register.

        Raises
        ------
        ValueError
            If order.id already contained within the registered stop-loss orders.

        """
        Condition.not_none(order, "order")
        Condition.not_in(order.cl_ord_id, self._stop_loss_ids, "order.id", "_stop_loss_ids")

        self._stop_loss_ids.add(order.cl_ord_id)
        self.log.debug(f"Registered SL order {order}")

    cpdef void register_take_profit(self, PassiveOrder order):
        """
        Register the given order to be managed as a take-profit.

        Parameters
        ----------
        order : PassiveOrder
            The take-profit order to register.

        Raises
        ------
        ValueError
            If order.id already contained within the registered take_profit orders.

        """
        Condition.not_none(order, "order")
        Condition.not_in(order.cl_ord_id, self._take_profit_ids, "order.cl_ord_id", "_take_profit_ids")

        self._take_profit_ids.add(order.cl_ord_id)
        self.log.debug(f"Registered TP order {order}")

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
                if self.reraise_exceptions:
                    raise ex  # Re-raise

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
                if self.reraise_exceptions:
                    raise ex  # Re-raise

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
                if self.reraise_exceptions:
                    raise ex  # Re-raise

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
                if self.reraise_exceptions:
                    raise ex  # Re-raise

    cpdef void handle_event(self, Event event) except *:
        """
        Hand the given event.

        Parameters
        ----------
        event : Event
            The received event.

        """
        Condition.not_none(event, "event")

        if isinstance(event, OrderRejected):
            self.log.warning(f"{RECV}{EVT} {event}.")
            if self.flatten_on_reject:
                self._flatten_on_reject(event)
        elif isinstance(event, OrderCancelReject):
            self.log.warning(f"{RECV}{EVT} {event}.")
        elif isinstance(event, PositionClosed):
            self._flattening_ids.discard(event.position.id)
        else:
            self.log.info(f"{RECV}{EVT} {event}.")

        # Remove order from registered orders
        if isinstance(event, _ORDER_COMPLETION_TRIGGERS):
            self._stop_loss_ids.discard(event.cl_ord_id)
            self._take_profit_ids.discard(event.cl_ord_id)

        if self._fsm.state == ComponentState.RUNNING:
            try:
                self.on_event(event)
            except Exception as ex:
                self.log.exception(ex)
                if self.reraise_exceptions:
                    raise ex  # Re-raise

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
            callback=self.handle_quote_ticks)

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

    cpdef bint has_quote_ticks(self, Symbol symbol):
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

    cpdef bint has_trade_ticks(self, Symbol symbol):
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

    cpdef bint has_bars(self, BarType bar_type):
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

    cpdef bint indicators_initialized(self):
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

# -- MANAGEMENT METHODS ----------------------------------------------------------------------------

    cpdef Account account(self):
        """
        Return the account for the strategy.

        Returns
        -------
        Account

        """
        Condition.not_none(self._exec_engine, "_exec_engine")

        return self._exec_engine.account

    cpdef Portfolio portfolio(self):
        """
        Return the portfolio for the strategy.

        Returns
        -------
        Portfolio

        """
        Condition.not_none(self._exec_engine, "_exec_engine")

        return self._exec_engine.portfolio

    cpdef OrderSide get_opposite_side(self, OrderSide side):
        """
        Return the opposite order side from the given side.

        Parameters
        ----------
        side : OrderSide
            The original order side.

        Returns
        -------
        OrderSide

        """
        Condition.not_equal(side, OrderSide.UNDEFINED, "side", "OrderSide.UNDEFINED")

        return OrderSide.BUY if side == OrderSide.SELL else OrderSide.SELL

    cpdef OrderSide get_flatten_side(self, PositionSide side):
        """
        Return the order side needed to flatten a position from the given side.

        Parameters
        ----------
        side : PositionSide
            The position side to flatten.

        Returns
        -------
        OrderSide

        Raises
        ------
        ValueError
            If side is UNDEFINED or FLAT.

        """
        Condition.not_equal(side, PositionSide.UNDEFINED, "side", "PositionSide.UNDEFINED")
        Condition.not_equal(side, PositionSide.FLAT, "side", "PositionSide.FLAT")

        return OrderSide.BUY if side == PositionSide.SHORT else OrderSide.SELL

    cpdef double get_exchange_rate(
            self,
            Currency from_currency,
            Currency to_currency,
            PriceType price_type=PriceType.MID,
    ):
        """
        Return the calculated exchange rate for the given currencies.

        Parameters
        ----------
        from_currency : Currency
            The currency to convert from.
        to_currency : Currency
            The currency to convert to.
        price_type : PriceType
            The price type for the exchange rate (default=MID).

        Returns
        -------
        double

        Raises
        ------
        ValueError
            If price_type is UNDEFINED or LAST.

        """
        Condition.not_none(self._data_engine, "data_client")

        return self._data_engine.get_exchange_rate(
            from_currency=from_currency,
            to_currency=to_currency,
            price_type=price_type)

    cpdef double get_exchange_rate_for_account(
            self,
            Currency quote_currency,
            PriceType price_type=PriceType.MID,
    ):
        """
        Return the calculated exchange rate for the give trading instrument quote
        currency to the account currency.

        Parameters
        ----------
        quote_currency : Currency
            The quote currency to convert from.
        price_type : PriceType
            The price type for the exchange rate (default=MID).

        Returns
        -------
        double

        Raises
        ------
        ValueError
            If price_type is UNDEFINED or LAST.

        """
        Condition.not_none(self._data_engine, "data_client")

        cdef Account account = self.account()
        if account is None:
            self.log.error("Cannot get exchange rate (account is not initialized).")
            return 0.0

        return self._data_engine.get_exchange_rate(
            from_currency=quote_currency,
            to_currency=self.account().currency,
            price_type=price_type)

    cpdef Order order(self, ClientOrderId cl_ord_id):
        """
        Return the order with the given identifier (if found).

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The client order identifier.

        Returns
        -------
        Order or None

        """
        Condition.not_none(cl_ord_id, "cl_ord_id")
        Condition.not_none(self._exec_engine, "_exec_engine")

        return self._exec_engine.database.get_order(cl_ord_id)

    cpdef dict orders(self, Symbol symbol=None):
        """
        Return a all orders associated with this strategy.

        symbol : Symbol, optional
            The symbol identifier query filter.

        Returns
        -------
        Dict[OrderId, Order]

        """
        Condition.not_none(self._exec_engine, "_exec_engine")

        return self._exec_engine.database.get_orders(symbol, self.id)

    cpdef dict orders_working(self, Symbol symbol=None):
        """
        Return all working orders associated with this strategy.

        symbol : Symbol, optional
            The symbol identifier query filter.

        Returns
        -----
        Dict[OrderId, Order]

        """
        Condition.not_none(self._exec_engine, "_exec_engine")

        return self._exec_engine.database.get_orders_working(symbol, self.id)

    cpdef set stop_loss_ids(self):
        """
        Return all working stop-loss orders associated with this strategy.

        Returns
        -------
        Set[OrderId]

        """
        return self._stop_loss_ids.copy()

    cpdef set take_profit_ids(self):
        """
        Return all working take-profit orders associated with this strategy.

        Returns
        -------
        Set[OrderId]

        """
        return self._take_profit_ids.copy()

    cpdef dict orders_completed(self, Symbol symbol=None):
        """
        Return all completed orders associated with this strategy.

        symbol : Symbol, optional
            The symbol identifier query filter.

        Returns
        -------
        Dict[OrderId, Order]

        """
        Condition.not_none(self._exec_engine, "_exec_engine")

        return self._exec_engine.database.get_orders_completed(symbol, self.id)

    cpdef int orders_working_count(self, Symbol symbol=None):
        """
        Return the count of working orders held by the execution engine.

        symbol : Symbol, optional
            The symbol identifier query filter.

        Returns
        -------
        int

        """
        Condition.not_none(self._exec_engine, "_exec_engine")

        return self._exec_engine.database.orders_working_count(symbol, self.id)

    cpdef int orders_completed_count(self, Symbol symbol=None):
        """
        Return the count of completed orders held by the execution engine.

        symbol : Symbol, optional
            The symbol identifier query filter.

        Returns
        -------
        int

        """
        Condition.not_none(self._exec_engine, "_exec_engine")

        return self._exec_engine.database.orders_completed_count(symbol, self.id)

    cpdef int orders_total_count(self, Symbol symbol=None):
        """
        Return the total count of orders held by the execution engine.

        symbol : Symbol, optional
            The symbol identifier query filter.

        Returns
        -------
        int

        """
        Condition.not_none(self._exec_engine, "_exec_engine")

        return self._exec_engine.database.orders_total_count(symbol, self.id)

    cpdef Position position(self, PositionId position_id):
        """
        Return the position associated with the given identifier (if found).

        Parameters
        ----------
        position_id : PositionId
            The position identifier.

        Returns
        -------
        Position or None

        """
        Condition.not_none(position_id, "position_id")
        Condition.not_none(self._exec_engine, "_exec_engine")

        return self._exec_engine.database.get_position(position_id)

    cpdef Position position_for_order(self, ClientOrderId cl_ord_id):
        """
        Return the position associated with the given order_id (if found).

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The client order identifier.

        Returns
        -------
        Position or None

        """
        Condition.not_none(cl_ord_id, "cl_ord_id")
        Condition.not_none(self._exec_engine, "_exec_engine")

        cdef PositionId position_id = self._exec_engine.database.get_position_id(cl_ord_id)
        # If position_id is None then .get_position(position_id) will return None

        return self._exec_engine.database.get_position(position_id)

    cpdef dict positions(self, Symbol symbol=None):
        """
        Return a dictionary of all positions associated with this strategy.

        symbol : Symbol, optional
            The symbol identifier query filter.

        Returns
        -------
        Dict[PositionId, Position]

        """
        Condition.not_none(self._exec_engine, "_exec_engine")

        return self._exec_engine.database.get_positions(symbol, self.id)

    cpdef dict positions_open(self, Symbol symbol=None):
        """
        Return a dictionary of all active positions associated with this strategy.

        symbol : Symbol, optional
            The symbol identifier query filter.

        Returns
        ------
        Dict[PositionId, Position]

        """
        Condition.not_none(self._exec_engine, "_exec_engine")

        return self._exec_engine.database.get_positions_open(symbol, self.id)

    cpdef dict positions_closed(self, Symbol symbol=None):
        """
        Return a dictionary of all closed positions associated with this strategy.

        symbol : Symbol, optional
            The symbol identifier query filter.

        Returns
        -------
        Dict[PositionId, Position]

        """
        Condition.not_none(self._exec_engine, "_exec_engine")

        return self._exec_engine.database.get_positions_closed(symbol, self.id)

    cpdef bint position_exists(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given identifier
        exists.

        Parameters
        ----------
        position_id : PositionId
            The position identifier.

        Returns
        -------
        bool

        """
        Condition.not_none(position_id, "position_id")
        Condition.not_none(self._exec_engine, "_exec_engine")

        return self._exec_engine.database.position_exists(position_id)

    cpdef int positions_open_count(self, Symbol symbol=None):
        """
        Return the count of open positions held by the execution engine.

        symbol : Symbol, optional
            The symbol identifier query filter.

        Returns
        -------
        int

        """
        Condition.not_none(self._exec_engine, "_exec_engine")

        return self._exec_engine.database.positions_open_count(symbol, self.id)

    cpdef int positions_closed_count(self, Symbol symbol=None):
        """
        Return the count of closed positions held by the execution engine.

        symbol : Symbol, optional
            The symbol identifier query filter.

        Returns
        -------
        int

        """
        Condition.not_none(self._exec_engine, "_exec_engine")

        return self._exec_engine.database.positions_closed_count(symbol, self.id)

    cpdef int positions_total_count(self, Symbol symbol=None):
        """
        Return the total count of positions held by the execution engine.

        symbol : Symbol, optional
            The symbol identifier query filter.

        Returns
        -------
        int

        """
        Condition.not_none(self._exec_engine, "_exec_engine")

        return self._exec_engine.database.positions_total_count(symbol, self.id)

    cpdef bint order_exists(self, ClientOrderId cl_ord_id):
        """
        Return a value indicating whether an order with the given identifier exists.

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The client order identifier.

        Returns
        -------
        bool

        """
        Condition.not_none(cl_ord_id, "cl_ord_id")
        Condition.not_none(self._exec_engine, "_exec_engine")

        return self._exec_engine.database.order_exists(cl_ord_id)

    cpdef bint is_stop_loss(self, ClientOrderId cl_ord_id):
        """
        Return a value indicating whether the order with the given identifier is
        a registered stop-loss.

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The client order identifier.

        Returns
        -------
        bool

        """
        Condition.not_none(cl_ord_id, "cl_ord_id")

        return cl_ord_id in self._stop_loss_ids

    cpdef bint is_take_profit(self, ClientOrderId cl_ord_id):
        """
        Return a value indicating whether the order with the given identifier is
        a registered take-profit.

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The client order identifier.

        Returns
        -------
        bool

        """
        Condition.not_none(cl_ord_id, "cl_ord_id")

        return cl_ord_id in self._take_profit_ids

    cpdef bint is_order_working(self, ClientOrderId cl_ord_id):
        """
        Return a value indicating whether an order with the given identifier is working.

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The client order identifier.

        Returns
        -------
        bool

        """
        Condition.not_none(cl_ord_id, "cl_ord_id")
        Condition.not_none(self._exec_engine, "_exec_engine")

        return self._exec_engine.database.is_order_working(cl_ord_id)

    cpdef bint is_order_completed(self, ClientOrderId cl_ord_id):
        """
        Return a value indicating whether an order with the given identifier is complete.

        Parameters
        ----------
        cl_ord_id : ClientOrderId
            The client order identifier.

        Returns
        -------
        bool

        """
        Condition.not_none(cl_ord_id, "cl_ord_id")
        Condition.not_none(self._exec_engine, "_exec_engine")

        return self._exec_engine.database.is_order_completed(cl_ord_id)

    cpdef bint is_position_open(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given identifier
        is open.

        Parameters
        ----------
        position_id : PositionId
            The position identifier.

        Returns
        -------
        bool

        """
        Condition.not_none(position_id, "position_id")
        Condition.not_none(self._exec_engine, "_exec_engine")

        return self._exec_engine.database.is_position_open(position_id)

    cpdef bint is_position_closed(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given identifier is closed.

        Parameters
        ----------
        position_id : PositionId
            The position identifier.

        Returns
        -------
        bool

        """
        Condition.not_none(position_id, "position_id")
        Condition.not_none(self._exec_engine, "_exec_engine")

        return self._exec_engine.database.is_position_closed(position_id)

    cpdef bint is_flat(self, Symbol symbol=None):
        """
        Return a value indicating whether the strategy is flat.

        symbol : Symbol, optional
            The symbol identifier query filter.

        Returns
        -------
        bool

        """
        Condition.not_none(self._exec_engine, "_exec_engine")

        return self._exec_engine.is_flat(symbol=symbol, strategy_id=self.id)

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void start(self) except *:
        """
        Start the trading strategy.

        Calls on_start().
        """
        try:
            self._fsm.trigger('START')
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

        self._fsm.trigger('RUNNING')
        self.log.info(f"state={self._fsm.state_as_string()}.")

    cpdef void stop(self) except *:
        """
        Stop the trading strategy.

        Calls on_stop().
        """
        try:
            self._fsm.trigger('STOP')
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

        # Flatten open positions
        if self.flatten_on_stop:
            self.flatten_all_positions()

        # Cancel working orders
        if self.cancel_all_orders_on_stop:
            self.cancel_all_orders()

        try:
            self.on_stop()
        except Exception as ex:
            self.log.exception(ex)

        self._fsm.trigger('STOPPED')
        self.log.info(f"state={self._fsm.state_as_string()}.")

    cpdef void resume(self) except *:
        """
        Resume the trading strategy.

        Calls on_resume().
        """
        try:
            self._fsm.trigger('RESUME')
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

        self._fsm.trigger('RUNNING')
        self.log.info(f"state={self._fsm.state_as_string()}.")

    cpdef void reset(self) except *:
        """
        Reset the trading strategy.

        Calls on_reset().
        All stateful values are reset to their initial value.
        """
        try:
            self._fsm.trigger('RESET')
        except InvalidStateTrigger as ex:
            self.log.exception(ex)
            return

        self.log.info(f"state={self._fsm.state_as_string()}...")

        if self.order_factory is not None:
            self.order_factory.reset()

        self._flattening_ids = set()   # type: {PositionId}
        self._stop_loss_ids = set()    # type: {ClientOrderId}
        self._take_profit_ids = set()  # type: {ClientOrderId}

        self._indicators.clear()
        self._indicators_for_quotes.clear()
        self._indicators_for_trades.clear()
        self._indicators_for_bars.clear()

        try:
            self.on_reset()
        except Exception as ex:
            self.log.exception(ex)

        self._fsm.trigger('RESET')
        self.log.info(f"state={self._fsm.state_as_string()}.")

    cpdef void dispose(self) except *:
        """
        Dispose of the trading strategy.
        """
        try:
            self._fsm.trigger('DISPOSE')
        except InvalidStateTrigger as ex:
            self.log.exception(ex)
            return

        self.log.info(f"state={self._fsm.state_as_string()}...")

        try:
            self.on_dispose()
        except Exception as ex:
            self.log.exception(ex)

        self._fsm.trigger('DISPOSED')
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

    cpdef void account_inquiry(self) except *:
        """
        Send an account inquiry command to the execution engine.
        """
        Condition.not_none(self._exec_engine, "_exec_engine")

        cdef AccountInquiry command = AccountInquiry(
            self.trader_id,
            self._exec_engine.account_id,
            self.uuid_factory.generate(),
            self.clock.utc_now())

        self.log.info(f"{CMD}{SENT} {command}.")
        self._exec_engine.execute(command)

    cpdef void submit_order(self, Order order, PositionId position_id=None) except *:
        """
        Send a submit order command with the given order to the execution engine.

        Parameters
        ----------
        order : Order
            The order to submit.
        position_id : PositionId, optional
            The position identifier to submit the order against. Only required
            if the execution engine is configured for OMS.HEDGING.

        """
        Condition.not_none(order, "order")
        Condition.not_none(self.trader_id, "trader_id")
        Condition.not_none(self._exec_engine, "exec_engine")

        if position_id is None:
            # Null object pattern
            position_id = PositionId.null()

        cdef SubmitOrder command = SubmitOrder(
            self.trader_id,
            self._exec_engine.account_id,
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
        Send a submit bracket order command with the given order to the
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

        if register:
            self.register_stop_loss(bracket_order.stop_loss)
            if bracket_order.has_take_profit:
                self.register_take_profit(bracket_order.take_profit)

        cdef SubmitBracketOrder command = SubmitBracketOrder(
            self.trader_id,
            self._exec_engine.account_id,
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
        Send a modify order command for the given order with the given new price
        to the execution engine.

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

        if new_quantity is None and new_price is None:
            self.log.error("Cannot send command ModifyOrder (both new_quantity and new_price were None).")
            return

        if new_quantity is None:
            new_quantity = order.quantity

        if new_price is None:
            new_price = order.price

        cdef ModifyOrder command = ModifyOrder(
            self.trader_id,
            self._exec_engine.account_id,
            order.cl_ord_id,
            new_quantity,
            new_price,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        self.log.info(f"{CMD}{SENT} {command}.")
        self._exec_engine.execute(command)

    cpdef void cancel_order(self, Order order) except *:
        """
        Send a cancel order command for the given order and cancel_reason to the
        execution engine.

        Parameters
        ----------
        order : Order
            The order to cancel.

        """
        Condition.not_none(order, "order")
        Condition.not_none(self.trader_id, "trader_id")
        Condition.not_none(self._exec_engine, "_exec_engine")

        cdef CancelOrder command = CancelOrder(
            self.trader_id,
            self._exec_engine.account_id,
            order.cl_ord_id,
            self.uuid_factory.generate(),
            self.clock.utc_now(),
        )

        self.log.info(f"{CMD}{SENT} {command}.")
        self._exec_engine.execute(command)

    cpdef void cancel_all_orders(self) except *:
        """
        Send a cancel order command for orders associated with this strategy
        which are not completed.
        """
        Condition.not_none(self._exec_engine, "_exec_engine")

        cdef dict working_orders = self._exec_engine.database.get_orders_working(
            symbol=None,
            strategy_id=self.id,
        )

        cdef int working_orders_count = len(working_orders)
        if working_orders_count == 0:
            self.log.info("No working orders to cancel.")
            return

        self.log.info(f"Cancelling {working_orders_count} working order(s)...")
        cdef ClientOrderId order_id
        cdef Order order
        cdef CancelOrder command
        for order_id, order in working_orders.items():
            command = CancelOrder(
                self.trader_id,
                self._exec_engine.account_id,
                order_id,
                self.uuid_factory.generate(),
                self.clock.utc_now(),
            )

            self.log.info(f"{CMD}{SENT} {command}.")
            self._exec_engine.execute(command)

    cpdef void flatten_position(self, PositionId position_id) except *:
        """
        Flatten the position.

        Generate and submit the required market order to the execution engine
        to flatten the position corresponding to the given client position
        identifier. If the position is None or already FLAT will log a warning.

        Parameters
        ----------
        position_id : PositionId
            The position identifier to flatten.

        Raises
        ------
        ValueError
            If position_id is not found.

        """
        Condition.not_none(position_id, "position_id")
        Condition.not_none(self._exec_engine, "_exec_engine")

        if position_id in self._flattening_ids:
            self.log.warning(f"Already flattening {position_id}.")
            return

        cdef Position position = self._exec_engine.database.get_position(position_id)
        if position is None:
            self.log.error(f"Cannot flatten position (cannot find {position_id} in cached positions.")
            return

        if position.is_closed():
            self.log.warning(f"Cannot flatten position (the position {position_id} was already closed).")
            return

        cdef Order order = self.order_factory.market(
            position.symbol,
            self.get_flatten_side(position.side),
            position.quantity,
        )

        self._flattening_ids.add(position_id)

        self.log.info(f"Flattening {position}...")
        self.submit_order(order, position_id)  # Assumption that market order won't be rejected

    cpdef void flatten_all_positions(self) except *:
        """
        Flatten all positions.

        Generate and submit the required market to the execution engine to
        flatten open positions. If no positions found or a position is None
        then will log a warning.
        """
        Condition.not_none(self._exec_engine, "_exec_engine")

        cdef dict positions = self._exec_engine.database.get_positions_open(symbol=None, strategy_id=self.id)
        cdef int open_positions_count = len(positions)
        if open_positions_count == 0:
            self.log.info("No open positions to flatten.")
            return

        self.log.info(f"Flattening {open_positions_count} open position(s)...")

        cdef PositionId position_id
        cdef Position position
        cdef Order order
        for position_id, position in positions.items():
            if position.is_closed():
                self.log.warning(f"Cannot flatten position (the position {position_id} was already FLAT.")
                continue

            order = self.order_factory.market(
                position.symbol,
                self.get_flatten_side(position.side),
                position.quantity)

            self.log.info(f"Flattening {position}...")
            self.submit_order(order, position_id)

    cdef void _flatten_on_reject(self, OrderRejected event) except *:
        if event.cl_ord_id not in self._stop_loss_ids and event.cl_ord_id not in self._take_profit_ids:
            return  # Not a registered stop-loss

        # Find position_id for order
        cdef PositionId position_id = self._exec_engine.database.get_position_id(event.cl_ord_id)
        if position_id is None:
            self.log.error(f"Cannot find PositionId for {event.cl_ord_id}.")
            return

        # Flatten if open position
        if self._exec_engine.database.is_position_open(position_id):
            self.log.warning(f"Rejected {event.cl_ord_id} was a registered child order, now flattening {position_id}.")
            self.flatten_position(position_id)
