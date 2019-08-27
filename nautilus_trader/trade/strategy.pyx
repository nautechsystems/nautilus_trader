# -------------------------------------------------------------------------------------------------
# <copyright file="strategy.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime
from collections import deque
from typing import List, Dict, Deque, Callable

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.types cimport ValidString
from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.c_enums.quote_type cimport QuoteType
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.market_position cimport MarketPosition
from nautilus_trader.model.currency cimport ExchangeRateCalculator
from nautilus_trader.model.events cimport Event, OrderRejected, OrderExpired, OrderCancelled
from nautilus_trader.model.events cimport OrderCancelReject, OrderModified, OrderFilled, OrderPartiallyFilled
from nautilus_trader.model.identifiers cimport Symbol, Label, TraderId, StrategyId, OrderId, PositionId
from nautilus_trader.model.generators cimport PositionIdGenerator
from nautilus_trader.model.objects cimport Price, Tick, BarType, Bar, Instrument
from nautilus_trader.model.order cimport Order, AtomicOrder, OrderFactory
from nautilus_trader.model.position cimport Position
from nautilus_trader.common.clock cimport Clock, LiveClock
from nautilus_trader.common.logger cimport Logger, LoggerAdapter
from nautilus_trader.common.execution cimport ExecutionEngine
from nautilus_trader.common.data cimport DataClient
from nautilus_trader.common.guid cimport GuidFactory, LiveGuidFactory
from nautilus_trader.model.commands cimport AccountInquiry, SubmitOrder, SubmitAtomicOrder, ModifyOrder, CancelOrder
from nautilus_trader.data.tools cimport IndicatorUpdater


cdef class TradingStrategy:
    """
    The base class for all trading strategies.
    """

    def __init__(self,
                 str order_id_tag='001',
                 bint flatten_on_sl_reject=True,
                 bint flatten_on_stop=True,
                 bint cancel_all_orders_on_stop=True,
                 int tick_capacity=1000,
                 int bar_capacity=1000,
                 Clock clock=LiveClock(),
                 GuidFactory guid_factory=LiveGuidFactory(),
                 Logger logger=None):
        """
        Initializes a new instance of the TradingStrategy class.

        :param order_id_tag: The order_id tag for the strategy (should be unique at trader level).
        :param flatten_on_sl_reject: The flag indicating whether the position with an
        associated stop order should be flattened if the order is rejected.
        :param flatten_on_stop: The flag indicating whether the strategy should
        be flattened on stop.
        :param cancel_all_orders_on_stop: The flag indicating whether all residual
        orders should be cancelled on stop.
        :param bar_capacity: The capacity for the internal bar deque(s).
        :param clock: The clock for the strategy.
        :param guid_factory: The GUID factory for the strategy.
        :param logger: The logger for the strategy (can be None).
        :raises ConditionFailed: If the id_tag_trader is not a valid string.
        :raises ConditionFailed: If the id_tag_strategy is not a valid string.
        :raises ConditionFailed: If the tick_capacity is not positive (> 0).
        :raises ConditionFailed: If the bar_capacity is not positive (> 0).
        """
        Condition.valid_string(order_id_tag, 'id_tag_strategy')
        Condition.positive(tick_capacity, 'tick_capacity')
        Condition.positive(bar_capacity, 'bar_capacity')

        # Identification
        self.trader_id = TraderId('TEST', '000')
        self.id = StrategyId(self.__class__.__name__, order_id_tag)

        # Components
        self.log = LoggerAdapter(self.id.value, logger)
        self._guid_factory = guid_factory
        self.clock = clock
        self.clock.register_logger(self.log)
        self.clock.register_handler(self.handle_event)

        # Order management flags
        self.flatten_on_sl_reject = flatten_on_sl_reject
        self.flatten_on_stop = flatten_on_stop
        self.cancel_all_orders_on_stop = cancel_all_orders_on_stop

        # Order / Position components
        self.order_factory = OrderFactory(
            id_tag_trader=self.trader_id.order_id_tag,
            id_tag_strategy=self.id.order_id_tag,
            clock=self.clock)
        self.position_id_generator = PositionIdGenerator(
            id_tag_trader=self.trader_id.order_id_tag,
            id_tag_strategy=self.id.order_id_tag,
            clock=self.clock)

        # Registered orders
        self._entry_orders = {}               # type: Dict[OrderId, Order]
        self._stop_loss_orders = {}           # type: Dict[OrderId, Order]
        self._take_profit_orders = {}         # type: Dict[OrderId, Order]
        self._atomic_order_ids = {}           # type: Dict[OrderId, List[OrderId]]

        # Data
        self.tick_capacity = tick_capacity
        self.bar_capacity = bar_capacity
        self._ticks = {}                      # type: Dict[Symbol, Deque[Tick]]
        self._bars = {}                       # type: Dict[BarType, Deque[Bar]]
        self._indicators = []                 # type: List[object]
        self._indicator_updaters_ticks = {}   # type: Dict[Symbol, List[IndicatorUpdater]]
        self._indicator_updaters_bars = {}    # type: Dict[BarType, List[IndicatorUpdater]]
        self._exchange_calculator = ExchangeRateCalculator()

        # Command buffers
        self._modify_order_buffer = {}   # type: Dict[OrderId, ModifyOrder]

        # Registered modules
        self._data_client = None  # Initialized when registered with the data client
        self._exec_engine = None  # Initialized when registered with the execution engine
        self.portfolio = None     # Initialized when registered with the execution engine
        self.account = None       # Initialized when registered with the execution engine

        self.is_running = False

    cdef bint equals(self, TradingStrategy other):
        """
        Return a value indicating whether this object is equal to the given object.

        :param other: The other object.
        :return bool.
        """
        return self.id.equals(other.id)

    def __eq__(self, TradingStrategy other) -> bool:
        """
        Return a value indicating whether this object is equal to the given object.

        :param other: The other object.
        :return bool.
        """
        return self.equals(other)

    def __ne__(self, TradingStrategy other) -> bool:
        """
        Return a value indicating whether this object is not equal to the given object.

        :param other: The other object.
        :return bool.
        """
        return not self.equals(other)

    def __hash__(self) -> int:
        """"
        Return a hash representation of this object.

        :return int.
        """
        return hash(self.id.value)

    def __str__(self) -> str:
        """
        Return a string representation of this object.

        :return str.
        """
        return f"{self.__class__.__name__}({self.id.value})"

    def __repr__(self) -> str:
        """
        Return a string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{str(self)} object at {id(self)}>"


#-- ABSTRACT METHODS -----------------------------------------------------------------------------#

    cpdef void on_start(self) except *:
        """
        Called when the strategy is started.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method on_start() must be implemented in the strategy (or just add pass).")

    cpdef void on_tick(self, Tick tick) except *:
        """
        Called when a tick is received by the strategy.

        :param tick: The tick received.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method on_tick() must be implemented in the strategy (or just add pass).")

    cpdef void on_bar(self, BarType bar_type, Bar bar) except *:
        """
        Called when a bar is received by the strategy.

        :param bar_type: The bar type received.
        :param bar: The bar received.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method on_bar() must be implemented in the strategy (or just add pass).")

    cpdef void on_instrument(self, Instrument bar_type) except *:
        """
        Called when an instrument update is received by the strategy.

        :param bar_type: The instrument received.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method on_instrument() must be implemented in the strategy (or just add pass).")

    cpdef void on_event(self, Event event) except *:
        """
        Called when an event is received by the strategy.

        :param event: The event received.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method on_event() must be implemented in the strategy (or just add pass).")

    cpdef void on_stop(self) except *:
        """
        Called when the strategy is stopped.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method on_stop() must be implemented in the strategy (or just add pass).")

    cpdef void on_reset(self) except *:
        """
        Called when the strategy is reset.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method reset() must be implemented in the strategy (or just add pass).")

    cpdef void on_dispose(self) except *:
        """
        Called when the strategy is disposed.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method on_dispose() must be implemented in the strategy (or just add pass).")


#-- REGISTRATION METHODS --------------------------------------------------------------------------#

    cpdef void register_trader(self, TraderId trader_id) except *:
        """
        Change the trader for the strategy.

        :param trader_id: The trader identifier to change to.
        """
        self.trader_id = trader_id
        self.log.debug(f"Registered trader {trader_id.value}.")

    cpdef void register_data_client(self, DataClient client) except *:
        """
        Register the strategy with the given data client.

        :param client: The data client to register.
        """
        self._data_client = client
        self.log.debug("Registered data client.")

    cpdef void register_execution_engine(self, ExecutionEngine engine) except *:
        """
        Register the strategy with the given execution engine.

        :param engine: The execution engine to register.
        """
        self._exec_engine = engine
        self.portfolio = engine.portfolio
        self.account = engine.account
        self.log.debug("Registered execution engine.")

    cpdef void register_indicator_ticks(
            self,
            Symbol symbol,
            indicator,
            update_method: Callable) except *:
        """
        Register the given indicator with the strategy. 
        It will receive ticks for the given symbol.

        :param symbol: The indicators symbol.
        :param indicator: The indicator to register.
        :param update_method: The update method for the indicator.
        :raises ConditionFailed: If the update_method is not of type Callable.
        """
        Condition.type(update_method, Callable, 'update_method')

        if indicator not in self._indicators:
            self._indicators.append(indicator)

        if symbol not in self._indicator_updaters_bars:
            self._indicator_updaters_bars[symbol] = []  # type: List[IndicatorUpdater]

        if indicator not in self._indicator_updaters_ticks[symbol]:
            self._indicator_updaters_ticks[symbol].append(IndicatorUpdater(indicator, update_method))
        else:
            self.log.error(f"Indicator {indicator} already registered for {symbol}.")

    cpdef void register_indicator_bars(
            self,
            BarType bar_type,
            indicator,
            update_method: Callable) except *:
        """
        Register the given indicator with the strategy. 
        It will receive bars of the given bar type.

        :param bar_type: The indicators bar type.
        :param indicator: The indicator to register.
        :param update_method: The update method for the indicator.
        :raises ConditionFailed: If the update_method is not of type Callable.
        """
        Condition.type(update_method, Callable, 'update_method')

        if indicator not in self._indicators:
            self._indicators.append(indicator)

        if bar_type not in self._indicator_updaters_bars:
            self._indicator_updaters_bars[bar_type] = []  # type: List[IndicatorUpdater]

        if indicator not in self._indicator_updaters_bars[bar_type]:
            self._indicator_updaters_bars[bar_type].append(IndicatorUpdater(indicator, update_method))
        else:
            self.log.error(f"Indicator {indicator} already registered for {bar_type}.")

    cpdef void register_entry_order(self, Order order, PositionId position_id) except *:
        """
        Register the given order as an entry order.
        
        :param order: The entry order to register.
        :param position_id: The position_id to associate with the entry order.
        """
        self._entry_orders[order.id] = order

    cpdef void register_stop_loss_order(self, Order order, PositionId position_id) except *:
        """
        Register the given order as a stop loss order for the given position_id.
        
        :param order: The stop loss order to register.
        :param position_id: The position_id to associate with the stop loss order.
        """
        self._stop_loss_orders[order.id] = order

    cpdef void register_take_profit_order(self, Order order, PositionId position_id) except *:
        """
        Register the given order as a take-profit order for the given position_id.

        :param order: The take-profit order to register.
        :param position_id: The position_id to associate with the take-profit order.
        """
        self._take_profit_orders[order.id] = order


#-- HANDLER METHODS -------------------------------------------------------------------------------#

    cpdef void handle_tick(self, Tick tick):
        """"
        System method. Update the internal ticks with the given tick, update
        indicators for the symbol, then call on_tick() and pass the tick 
        (if the strategy is running).

        :param tick: The tick received.
        """
        if tick.symbol not in self._ticks:
            self._ticks[tick.symbol] = deque(maxlen=self.tick_capacity)
        self._ticks[tick.symbol].appendleft(tick)

        cdef IndicatorUpdater updater
        if tick.symbol in self._indicator_updaters_ticks:
            for updater in self._indicator_updaters_ticks[tick.symbol]:
                updater.update_tick(tick)

        if self.is_running:
            try:
                self.on_tick(tick)
            except Exception as ex:
                self.log.exception(ex)

    cpdef void handle_ticks(self, list ticks):
        """
        System method. Handle the given list of ticks by handling each tick individually.
        """
        for i in range(len(ticks)):
            self.handle_tick(ticks[i])

    cpdef void handle_bar(self, BarType bar_type, Bar bar):
        """"
        System method. Update the internal bars with the given bar, update 
        indicators for the bar type, then call on_bar() and pass the arguments 
        (if the strategy is running).

        :param bar_type: The bar type received.
        :param bar: The bar received.
        """
        if bar_type not in self._bars:
            self._bars[bar_type] = deque(maxlen=self.bar_capacity)
        self._bars[bar_type].appendleft(bar)

        cdef IndicatorUpdater updater
        if bar_type in self._indicator_updaters_bars:
            for updater in self._indicator_updaters_bars[bar_type]:
                updater.update_bar(bar)

        if self.is_running:
            try:
                self.on_bar(bar_type, bar)
            except Exception as ex:
                self.log.exception(ex)

    cpdef void handle_bars(self, BarType bar_type, list bars):
        """
        System method. Handle the given bar type and bars by handling 
        each bar individually.
        """
        self.log.info(f"Received bar data for {bar_type} of {len(bars)} bars.")

        for i in range(len(bars)):
            self.handle_bar(bar_type, bars[i])

    cpdef void handle_instrument(self, Instrument instrument):
        """
        System method. Handle the given instrument.
        """
        if self.is_running:
            try:
                self.on_instrument(instrument)
            except Exception as ex:
                self.log.exception(ex)

    cpdef void handle_event(self, Event event):
        """
        Call on_event() and passes the event (if the strategy is running).

        :param event: The event received.
        """
        cdef Position position
        if isinstance(event, OrderRejected):
            self.log.warning(f"{event}")
            if event.order_id in self._stop_loss_orders and self.flatten_on_sl_reject:
                position = self._exec_engine.database.get_position_for_order(event.order_id)
                if position is not None:
                    self.log.critical(f"Rejected order {event.order_id} was a registered stop-loss. Flattening entered position {position.id}.")
                    self.flatten_position(position.id)
                else:
                    self.log.critical(f"Rejected order {event.order_id} was a registered stop-loss. Position {position.id} not entered.")
            self._remove_atomic_child_orders(event.order_id)
            self._remove_from_registered_orders(event.order_id)

        elif isinstance(event, OrderCancelled):
            self.log.info(f"{event}")
            self._remove_atomic_child_orders(event.order_id)
            self._remove_from_registered_orders(event.order_id)

        elif isinstance(event, OrderFilled):
            self.log.info(f"{event}")
            if event.order_id in self._atomic_order_ids:
                del self._atomic_order_ids[event.order_id]
            self._remove_from_registered_orders(event.order_id)

        elif isinstance(event, OrderPartiallyFilled):
            self.log.warning(f"{event}")

        elif isinstance(event, OrderCancelReject):
            self.log.warning(f"{event}")
            if event.order_id in self._modify_order_buffer:
                self._process_modify_order_buffer(event.order_id)

        elif isinstance(event, OrderModified):
            self.log.info(f"{event}")
            if event.order_id in self._modify_order_buffer:
                self._process_modify_order_buffer(event.order_id)

        elif isinstance(event, OrderExpired):
            self.log.info(f"{event}")
            self._remove_atomic_child_orders(event.order_id)
            self._remove_from_registered_orders(event.order_id)

        else:
            self.log.info(f"{event}")

        if self.is_running:
            try:
                self.on_event(event)
            except Exception as ex:
                self.log.exception(ex)

    cdef void _remove_atomic_child_orders(self, OrderId order_id):
        # Remove any atomic child orders associated with the given order_id
        if order_id in self._atomic_order_ids:
            for child_order_id in self._atomic_order_ids[order_id]:
                self._remove_from_registered_orders(child_order_id)
            del self._atomic_order_ids[order_id]

    cdef void _remove_from_registered_orders(self, OrderId order_id):
        # Remove the given order_id from any registered order dictionary
        if order_id in self._entry_orders:
            del self._entry_orders[order_id]
        elif order_id in self._stop_loss_orders:
            del self._stop_loss_orders[order_id]
        elif order_id in self._take_profit_orders:
            del self._take_profit_orders[order_id]

    cdef void _process_modify_order_buffer(self, OrderId order_id):
        # Process the modify order buffer by checking if the commands modify order
        # price is not equal to the orders current price, in this case the buffered
        # command is sent for execution.
        cdef ModifyOrder buffered_command = self._modify_order_buffer[order_id]
        if buffered_command.modified_price != self.order(order_id).price:
            self.log.info(f"Modifying {buffered_command.order_id} with price {buffered_command.modified_price}")
            self._exec_engine.execute_command(buffered_command)
        del self._modify_order_buffer[order_id]


#-- DATA METHODS ----------------------------------------------------------------------------------#

    cpdef datetime time_now(self):
        """
        Return the current time from the strategies internal clock (UTC).
        
        :return datetime.
        """
        return self.clock.time_now()

    cpdef list instrument_symbols(self):
        """
        Return all instruments held by the data client.
        
        :return List[Instrument].
        :raises ConditionFailed: If the strategy has not been registered with a data client.
        """
        Condition.not_none(self._data_client, 'data_client')

        return self._data_client.instrument_symbols()

    cpdef Instrument get_instrument(self, Symbol symbol):
        """
        Return the instrument corresponding to the given symbol.

        :param symbol: The symbol of the instrument to return.
        :return Instrument (if found) or None.
        :raises ConditionFailed: If strategy has not been registered with a data client.
        :raises ConditionFailed: If the instrument is not found.
        """
        Condition.not_none(self._data_client, 'data_client')

        return self._data_client.get_instrument(symbol)

    cpdef dict instruments_all(self):
        """
        Return a dictionary of all instruments held by the data client.
        
        :return Dict[Symbol, Instrument].
        """
        return self._data_client.get_instruments_all()

    cpdef void request_bars(self, BarType bar_type, datetime from_datetime=None, datetime to_datetime=None):
        """
        Request the historical bars for the given parameters from the data service.

        Note: Logs warning if the downloaded bars 'from' datetime is greater than that given.

        :param bar_type: The historical bar type to download.
        :param from_datetime: The datetime from which the historical bars should be downloaded.
        :param to_datetime: The datetime to which the historical bars should be downloaded.
        :raises ConditionFailed: If the from_datetime is not None and not less than to_datetime.
        """
        if to_datetime is None:
            to_datetime = self.clock.time_now()
        if from_datetime is None:
            from_datetime = to_datetime - (self.bar_capacity * bar_type.specification.timedelta())

        Condition.true(from_datetime < to_datetime, 'from_datetime < to_date')

        if self._data_client is None:
            self.log.error("Cannot download historical bars (data client not registered).")
            return

        self._data_client.request_bars(bar_type, from_datetime, to_datetime, self.handle_bars)

    cpdef void subscribe_ticks(self, Symbol symbol):
        """
        Subscribe to tick data for the given symbol.

        :param symbol: The tick symbol to subscribe to.
        """
        if self._data_client is None:
            self.log.error("Cannot subscribe to ticks (data client not registered).")
            return

        self._data_client.subscribe_ticks(symbol, self.handle_tick)
        self.log.info(f"Subscribed to tick data for {symbol}.")

    cpdef void subscribe_bars(self, BarType bar_type):
        """
        Subscribe to bar data for the given bar type.

        :param bar_type: The bar type to subscribe to.
        """
        if self._data_client is None:
            self.log.error("Cannot subscribe to bars (data client not registered).")
            return

        self._data_client.subscribe_bars(bar_type, self.handle_bar)
        self.log.info(f"Subscribed to bar data for {bar_type}.")

    cpdef void subscribe_instrument(self, Symbol symbol):
        """
        Subscribe to instrument data for the given symbol.

        :param symbol: The instrument symbol to subscribe to.
        """
        if self._data_client is None:
            self.log.error("Cannot subscribe to instrument (data client not registered).")
            return

        self._data_client.subscribe_instrument(symbol, self.handle_instrument)
        self.log.info(f"Subscribed to instrument data for {symbol}.")

    cpdef void unsubscribe_ticks(self, Symbol symbol):
        """
        Unsubscribe from tick data for the given symbol.

        :param symbol: The tick symbol to unsubscribe from.
        """
        if self._data_client is None:
            self.log.error("Cannot unsubscribe from ticks (data client not registered).")
            return

        self._data_client.unsubscribe_ticks(symbol, self.handle_tick)
        self.log.info(f"Unsubscribed from tick data for {symbol}.")

    cpdef void unsubscribe_bars(self, BarType bar_type):
        """
        Unsubscribe from bar data for the given bar type.

        :param bar_type: The bar type to unsubscribe from.
        """
        if self._data_client is None:
            self.log.error("Cannot unsubscribe from bars (data client not registered).")
            return

        self._data_client.unsubscribe_bars(bar_type, self.handle_bar)
        self.log.info(f"Unsubscribed from bar data for {bar_type}.")

    cpdef void unsubscribe_instrument(self, Symbol symbol):
        """
        Unsubscribe from instrument data for the given symbol.

        :param symbol: The instrument symbol to unsubscribe from.
        """
        if self._data_client is None:
            self.log.error("Cannot unsubscribe from instrument (data client not registered).")
            return

        self._data_client.unsubscribe_instrument(symbol, self.handle_instrument)
        self.log.info(f"Unsubscribed from instrument data for {symbol}.")

    cpdef bint has_ticks(self, Symbol symbol):
        """
        Return a value indicating whether the strategy has ticks for the given symbol.
        
        :param symbol: The symbol of the ticks.
        :return bool.
        """
        return symbol in self._ticks and len(self._ticks[symbol]) > 0

    cpdef bint has_bars(self, BarType bar_type):
        """
        Return a value indicating whether the strategy has bars for the given bar type.
        
        :param bar_type: The bar_type of the bars.
        :return bool.
        """
        return bar_type in self._bars and len(self._bars[bar_type]) > 0

    cpdef int tick_count(self, Symbol symbol):
        """
        Return the count of ticks held by the strategy for the given symbol.
        
        :param symbol: The tick symbol to count.
        :return int.
        :raises ConditionFailed: If the strategies tick dictionary does not contain the symbol.
        """
        Condition.is_in(symbol, self._ticks, 'symbol', 'ticks')

        return len(self._ticks[symbol])

    cpdef int bar_count(self, BarType bar_type):
        """
        Return the count of ticks held by the strategy for the given symbol.
        
        :param bar_type: The bar type to count.
        :return int.
        :raises ConditionFailed: If the strategies bars dictionary does not contain the bar type.
        """
        Condition.is_in(bar_type, self._bars, 'bar_type', 'bars')

        return len(self._bars[bar_type])

    cpdef list ticks(self, Symbol symbol):
        """
        Return the ticks for the given symbol (returns a copy of the internal deque).

        :param symbol: The symbol for the ticks to get.
        :return List[Tick].
        :raises ConditionFailed: If the strategies tick dictionary does not contain the symbol.
        """
        Condition.is_in(symbol, self._ticks, 'symbol', 'ticks')

        return list(self._ticks[symbol])

    cpdef list bars(self, BarType bar_type):
        """
        Return the bars for the given bar type (returns a copy of the internal deque).

        :param bar_type: The bar type to get.
        :return List[Bar].
        :raises ConditionFailed: If the strategies bars dictionary does not contain the bar type.
        """
        Condition.is_in(bar_type, self._bars, 'bar_type', 'bars')

        return list(self._bars[bar_type])

    cpdef Tick tick(self, Symbol symbol, int index):
        """
        Return the tick for the given symbol at the given index.

        :param symbol: The symbol for the tick to get.
        :param index: The index for the tick to get.
        :return Tick.
        :raises ConditionFailed: If the strategies tick dictionary does not contain the symbol.
        :raises IndexError: If the tick index is out of range.
        """
        Condition.is_in(symbol, self._ticks, 'symbol', 'ticks')

        return self._ticks[symbol][index]

    cpdef Bar bar(self, BarType bar_type, int index):
        """
        Return the bar for the given bar type at the given index.

        :param bar_type: The bar type to get.
        :param index: The index for the bar to get.
        :return Bar.
        :raises ConditionFailed: If the strategies bars dictionary does not contain the bar type.
        :raises IndexError: If the bar index is out of range.
        """
        Condition.is_in(bar_type, self._bars, 'bar_type', 'bars')

        return self._bars[bar_type][index]


#-- INDICATOR METHODS -----------------------------------------------------------------------------#

    cpdef list registered_indicators(self):
        """
        Return the registered indicators for the strategy (returns copy).
        
        :return List[Indicator].
        """
        return self._indicators.copy()

    cpdef bint indicators_initialized(self):
        """
        Return a value indicating whether all indicators are initialized.

        :return bool.
        """
        for indicator in self._indicators:
            if indicator.initialized is False:
                return False
        return True


#-- MANAGEMENT METHODS ----------------------------------------------------------------------------#

    cpdef OrderSide get_opposite_side(self, OrderSide side):
        """
        Return the opposite order side from the given side.

        :param side: The original order side.
        :return OrderSide.
        """
        return OrderSide.BUY if side == OrderSide.SELL else OrderSide.SELL

    cpdef OrderSide get_flatten_side(self, MarketPosition market_position):
        """
        Return the order side needed to flatten a position from the given market position.

        :param market_position: The market position to flatten.
        :return OrderSide.
        :raises ConditionFailed: If the given market position is FLAT.
        """
        if market_position is MarketPosition.LONG:
            return OrderSide.SELL
        elif market_position is MarketPosition.SHORT:
            return OrderSide.BUY
        else:
            raise ValueError("Cannot flatten a FLAT position.")

    cpdef float get_exchange_rate(self, Currency quote_currency):
        """
        Return the calculated exchange rate for the give quote currency to the 
        account base currency.
        
        :param quote_currency: The quote currency for the exchange rate.
        :return float.
        """
        cdef dict bid_rates = {}
        cdef dict ask_rates = {}
        for symbol, ticks in self._ticks.items():
            bid_rates[symbol.code] = ticks[0].bid.as_float()
            ask_rates[symbol.code] = ticks[0].ask.as_float()

        return self._exchange_calculator.get_rate(
            quote_currency=quote_currency,
            base_currency=self.account.currency,
            quote_type=QuoteType.MID,
            bid_rates=bid_rates,
            ask_rates=ask_rates)

    cpdef Order order(self, OrderId order_id):
        """
        Return the order with the given identifier.

        :param order_id: The order_id.
        :return Order or None.
        """
        return self._exec_engine.database.get_order(order_id)

    cpdef dict orders(self):
        """
        Return a dictionary of all orders associated with this strategy.
        
        :return Dict[OrderId, Order].
        """
        return self._exec_engine.database.get_orders(self.id)

    cpdef dict orders_working(self):
        """
        Return a dictionary of all active orders associated with this strategy.
        
        :return Dict[OrderId, Order].
        """
        return self._exec_engine.database.get_orders_working(self.id)

    cpdef dict orders_completed(self):
        """
        Return a dictionary of all completed orders associated with this strategy.
        
        :return Dict[OrderId, Order].
        """
        return self._exec_engine.database.get_orders_completed(self.id)

    cpdef dict entry_orders(self):
        """
        Return a dictionary of pending or active entry orders.
        
        :return Dict[OrderId, Order].
        """
        return self._entry_orders

    cpdef dict stop_loss_orders(self):
        """
        Return a dictionary of pending or active stop loss orders with their 
        associated position_ids.
        
        :return Dict[OrderId, Order].
        """
        return self._stop_loss_orders

    cpdef dict take_profit_orders(self):
        """
        Return a dictionary of pending or active stop loss orders with their 
        associated position_ids.
        
        :return Dict[OrderId, Order].
        """
        return self._take_profit_orders

    cpdef list entry_order_ids(self):
        """
        Return a list of pending entry order_ids.

        :return List[OrderId].
        """
        return list(self._entry_orders.keys())

    cpdef list stop_loss_order_ids(self):
        """
        Return a list of stop-loss order_ids.

        :return List[OrderId].
        """
        return list(self._stop_loss_orders.keys())

    cpdef list take_profit_order_ids(self):
        """
        Return a list of stop-loss order_ids.

        :return List[OrderId].
        """
        return list(self._take_profit_orders.keys())

    cpdef Order entry_order(self, OrderId order_id):
        """
        Return the entry order associated with the given identifier (if found).

        :param order_id: The entry order_id.
        :return Order.
        :raises ConditionFailed. If the order_id is not registered with an entry.
        """
        Condition.is_in(order_id, self._entry_orders, 'order_id', 'pending_entry_orders')

        return self._entry_orders[order_id].order

    cpdef Order stop_loss_order(self, OrderId order_id):
        """
        Return the stop-loss order associated with the given identifier (if found).

        :param order_id: The stop-loss order_id.
        :return Order.
        :raises ConditionFailed. If the order_id is not registered with a stop-loss.
        """
        Condition.is_in(order_id, self._stop_loss_orders, 'order_id', 'stop_loss_orders')

        return self._stop_loss_orders[order_id].order

    cpdef Order take_profit_order(self, OrderId order_id):
        """
        Return the take-profit order associated with the given identifier (if found).

        :param order_id: The take-profit order_id.
        :return Order.
        :raises ConditionFailed. If the order_id is not registered with a take-profit.
        """
        Condition.is_in(order_id, self._take_profit_orders, 'order_id', 'take_profit_orders')

        return self._take_profit_orders[order_id].order

    cpdef Position position(self, PositionId position_id):
        """
        Return the position associated with the given position_id.

        :param position_id: The positions identifier.
        :return The position with the given identifier.
        :raises ConditionFailed: If the portfolio does not contain a position with the given identifier.
        """
        return self._exec_engine.database.get_position(position_id)

    cpdef dict positions(self):
        """
        Return a dictionary of all positions associated with this strategy.
        
        :return Dict[PositionId, Position]
        """
        return self._exec_engine.database.get_positions(self.id)

    cpdef dict positions_open(self):
        """
        Return a dictionary of all active positions associated with this strategy.
        
        :return Dict[PositionId, Position]
        """
        return self._exec_engine.database.get_positions_open(self.id)

    cpdef dict positions_closed(self):
        """
        Return a dictionary of all closed positions associated with this strategy.
        
        :return Dict[PositionId, Position]
        """
        return self._exec_engine.database.get_positions_closed(self.id)

    cpdef bint position_exists(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given identifier exists.
        
        :param position_id: The position_id.
        :return bool.
        """
        return self._exec_engine.database.position_exists(position_id)

    cpdef bint order_exists(self, OrderId order_id):
        """
        Return a value indicating whether an order with the given identifier exists.
        
        :param order_id: The order_id.
        :return bool.
        """
        return self._exec_engine.database.order_exists(order_id)

    cpdef bint is_order_working(self, OrderId order_id):
        """
        Return a value indicating whether an order with the given identifier is working.
         
        :param order_id: The order_id.
        :return bool.
        """
        return self._exec_engine.database.is_order_working(order_id)

    cpdef bint is_order_complete(self, OrderId order_id):
        """
        Return a value indicating whether an order with the given identifier is complete.
         
        :param order_id: The order_id.
        :return bool.
        """
        return self._exec_engine.database.is_order_completed(order_id)

    cpdef bint is_flat(self):
        """
        Return a value indicating whether the strategy is completely flat (i.e no market positions
        other than FLAT across all instruments).
        
        :return bool.
        """
        return self._exec_engine.is_strategy_flat(self.id)

    cpdef int entry_orders_count(self):
        """
        Return the count of pending entry orders registered with the strategy.
        
        :return int.
        """
        return len(self._entry_orders)

    cpdef int stop_loss_orders_count(self):
        """
        Return the count of stop-loss orders registered with the strategy.
        
        :return int.
        """
        return len(self._stop_loss_orders)

    cpdef int take_profit_orders_count(self):
        """
        Return the count of take-profit orders registered with the strategy.
        
        :return int.
        """
        return len(self._take_profit_orders)


#-- COMMANDS --------------------------------------------------------------------------------------#

    cpdef void start(self):
        """
        Start the trade strategy and call on_start().
        """
        self.log.info(f"Starting...")

        try:
            self.on_start()
        except Exception as ex:
            self.log.exception(ex)

        self.is_running = True
        self.log.info(f"Running...")

    cpdef void stop(self):
        """
        Stop the trade strategy and call on_stop().
        """
        self.log.info(f"Stopping...")

        # Clean up clock
        self.clock.cancel_all_time_alerts()
        self.clock.cancel_all_timers()

        # Clean up positions
        if self.flatten_on_stop:
            if not self.is_flat():
                self.flatten_all_positions()

        # Clean up orders
        if self.cancel_all_orders_on_stop:
            self.cancel_all_orders("STOPPING STRATEGY")

        # Check for residual objects
        for order in self._entry_orders.values():
            self.log.warning(f"Residual entry {order}")

        for order in self._stop_loss_orders.values():
            self.log.warning(f"Residual stop-loss {order}")

        for order in self._take_profit_orders.values():
            self.log.warning(f"Residual take-profit {order}")

        for order_id in self._atomic_order_ids:
            for child_order_id in self._atomic_order_ids[order_id]:
                self.log.warning(f"Residual child order {child_order_id}")

        # Check for residual buffered commands
        for order_id, command in self._modify_order_buffer.items():
            self.log.warning(f"Residual buffered {command} command for {order_id}.")

        try:
            self.on_stop()
        except Exception as ex:
            self.log.exception(ex)

        self.is_running = False
        self.log.info(f"Stopped.")

    cpdef void reset(self):
        """
        Reset the strategy by returning all stateful internal values to their
        initial value, the on_reset() implementation is then called. 
        
        Note: The strategy cannot be running otherwise an error is logged.
        """
        if self.is_running:
            self.log.error(f"Cannot reset (cannot reset a running strategy).")
            return

        self.log.info(f"Resetting...")
        self.order_factory.reset()
        self.position_id_generator.reset()
        self._ticks = {}                      # type: Dict[Symbol, Deque[Tick]]
        self._bars = {}                       # type: Dict[BarType, Deque[Bar]]
        self._indicators = []                 # type: List[object]
        self._indicator_updaters_ticks = {}   # type: Dict[Symbol, List[IndicatorUpdater]]
        self._indicator_updaters_bars = {}    # type: Dict[BarType, List[IndicatorUpdater]]

        for indicator in self._indicators:
            indicator.reset()

        try:
            self.on_reset()
        except Exception as ex:
            self.log.exception(ex)

        self.log.info(f"Reset.")

    cpdef void dispose(self):
        """
        Dispose of the strategy to release system resources, then call on_dispose().
        """
        self.log.info(f"Disposing...")

        try:
            self.on_dispose()
        except Exception as ex:
            self.log.error(str(ex))

        self.log.info(f"Disposed.")

    cpdef void account_inquiry(self):
        """
        Send an account inquiry command to the execution service.
        """
        if self._exec_engine is None:
            self.log.error("Cannot send account inquiry (execution engine not registered).")
            return

        cdef AccountInquiry command = AccountInquiry(
            self.account.id,
            self._guid_factory.generate(),
            self.clock.time_now())

        self._exec_engine.execute_command(command)

    cpdef void submit_order(self, Order order, PositionId position_id):
        """
        Send a submit order command with the given order and position_id to the execution 
        service.

        :param order: The order to submit.
        :param position_id: The position_id to associate with this order.
        """
        Condition.not_none(self._exec_engine, 'exec_engine')

        self.log.info(f"Submitting {order} with {position_id}")

        cdef SubmitOrder command = SubmitOrder(
            self.trader_id,
            self.id,
            position_id,
            self.account.id,
            order,
            self._guid_factory.generate(),
            self.clock.time_now())

        self._exec_engine.execute_command(command)

    cpdef void submit_entry_order(self, Order order, PositionId position_id):
        """
        Register the given order as an entry and then send a submit order command 
        with the given order and position_id to the execution service.
        """
        self.register_entry_order(order, position_id)
        self.submit_order(order, position_id)

    cpdef void submit_stop_loss_order(self, Order order, PositionId position_id):
        """
        Register the given order as a stop-loss and then send a submit order command 
        with the given order and position_id to the execution service.
        """
        self.register_stop_loss_order(order, position_id)
        self.submit_order(order, position_id)

    cpdef void submit_take_profit_order(self, Order order, PositionId position_id):
        """
        Register the given order as a take-profit and then send a submit order command 
        with the given order and position_id to the execution service.
        """
        self.register_take_profit_order(order, position_id)
        self.submit_order(order, position_id)

    cpdef void submit_atomic_order(self, AtomicOrder atomic_order, PositionId position_id):
        """
        Send a submit atomic order command with the given order and position_id to the 
        execution service.
        
        :param atomic_order: The atomic order to submit.
        :param position_id: The position_id to associate with this order.
        """
        if self._exec_engine is None:
            self.log.error("Cannot submit atomic order (execution engine not registered).")
            return

        self.log.info(f"Submitting {atomic_order} for {position_id}")

        self.register_entry_order(atomic_order.entry, position_id)
        self.register_stop_loss_order(atomic_order.stop_loss, position_id)

        if atomic_order.has_take_profit:
            self.register_take_profit_order(atomic_order.take_profit, position_id)

        # Track atomic order_ids
        cdef list child_order_ids = [atomic_order.stop_loss.id]
        if atomic_order.has_take_profit:
            child_order_ids.append(atomic_order.take_profit.id)
        self._atomic_order_ids[atomic_order.entry.id] = child_order_ids

        cdef SubmitAtomicOrder command = SubmitAtomicOrder(
            self.trader_id,
            self.id,
            position_id,
            self.account.id,
            atomic_order,
            self._guid_factory.generate(),
            self.clock.time_now())

        self._exec_engine.execute_command(command)

    cpdef void modify_order(self, Order order, Price new_price):
        """
        Send a modify order command for the given order with the given new price
        to the execution service.

        :param order: The order to modify.
        :param new_price: The new price for the given order.
        """
        if self._exec_engine is None:
            self.log.error("Cannot modify order (execution engine not registered).")
            return

        cdef ModifyOrder command = ModifyOrder(
            self.trader_id,
            self.id,
            self.account.id,
            order.id,
            new_price,
            self._guid_factory.generate(),
            self.clock.time_now())

        if order.id in self._modify_order_buffer:
            self._modify_order_buffer[order.id] = command
            self.log.warning(f"Buffering ModifyOrder command for {order} with new price {new_price}")
            return

        self._modify_order_buffer[order.id] = command
        self.log.info(f"Modifying {order} with new price {new_price}")

        self._exec_engine.execute_command(command)

    cpdef void cancel_order(self, Order order, str cancel_reason=None):
        """
        Send a cancel order command for the given order and cancel_reason to the
        execution service.

        :param order: The order to cancel.
        :param cancel_reason: The reason for cancellation (will be logged).
        :raises ConditionFailed: If the strategy has not been registered with an execution client.
        """
        if self._exec_engine is None:
            self.log.error("Cannot cancel order (execution client not registered).")
            return

        self.log.info(f"Cancelling {order}")

        cdef CancelOrder command = CancelOrder(
            self.trader_id,
            self.id,
            self.account.id,
            order.id,
            ValidString(cancel_reason),
            self._guid_factory.generate(),
            self.clock.time_now())

        self._exec_engine.execute_command(command)

    cpdef void cancel_all_orders(self, str cancel_reason=None):
        """
        Send a cancel order command for all currently working orders in the
        order book with the given cancel_reason - to the execution service.

        :param cancel_reason: The reason for cancellation (will be logged).
        :raises ConditionFailed: If the cancel_reason is not a valid string.
        """
        if self._exec_engine is None:
            self.log.error("Cannot cancel all orders (execution client not registered).")
            return

        cdef dict all_orders = self._exec_engine.database.get_orders(self.id)
        cdef CancelOrder command

        for order_id, order in all_orders.items():
            if order.is_working:
                command = CancelOrder(
                    self.trader_id,
                    self.id,
                    self.account.id,
                    order_id,
                    ValidString(cancel_reason),
                    self._guid_factory.generate(),
                    self.clock.time_now())

                self._exec_engine.execute_command(command)

    cpdef void flatten_position(self, PositionId position_id):
        """
        Flatten the position corresponding to the given identifier by generating
        the required market order, and sending it to the execution service.
        If the position is None or already FLAT will log a warning.

        :param position_id: The position_id to flatten.
        :raises ConditionFailed: If the position_id is not found in the position book.
        """
        if self._exec_engine is None:
            self.log.error("Cannot flatten position (execution client not registered).")
            return

        cdef Position position = self._exec_engine.database.get_position(position_id)

        if position.is_flat:
            self.log.warning(f"Cannot flatten position (the position {position_id} was already FLAT).")
            return

        cdef Order order = self.order_factory.market(
            position.symbol,
            self.get_flatten_side(position.market_position),
            position.quantity,
            Label("EXIT"),)

        self.log.info(f"Flattening {position}")
        self.submit_order(order, position_id)

    cpdef void flatten_all_positions(self):
        """
        Flatten all positions by generating the required market orders and sending
        them to the execution service. If no positions found or a position is None
        then will log a warning.
        """
        if self._exec_engine is None:
            self.log.error("Cannot flatten all positions (execution client not registered).")
            return

        cdef dict positions = self._exec_engine.database.get_positions_open(self.id)

        if len(positions) == 0:
            self.log.warning("Did not flatten all positions (no active positions to flatten).")
            return

        cdef PositionId position_id
        cdef Position position
        cdef Order order
        for position_id, position in positions.items():
            if position.is_flat:
                self.log.warning(f"Cannot flatten position (the position {position_id} was already FLAT.")
                continue

            order = self.order_factory.market(
                position.symbol,
                self.get_flatten_side(position.market_position),
                position.quantity,
                Label("EXIT"))

            self.log.info(f"Flattening {position}.")
            self.submit_order(order, position_id)


#-- BACKTEST METHODS ------------------------------------------------------------------------------#

    cpdef void change_clock(self, Clock clock):
        """
        Backtest only method. Change the strategies clock with the given clock.
        
        :param clock: The clock to change to.
        """
        self.clock = clock
        self.clock.register_logger(self.log)
        self.clock.register_handler(self.handle_event)

        self.order_factory = OrderFactory(
            id_tag_trader=self.trader_id.order_id_tag,
            id_tag_strategy=self.id.order_id_tag,
            clock=clock)

        self.position_id_generator = PositionIdGenerator(
            id_tag_trader=self.trader_id.order_id_tag,
            id_tag_strategy=self.id.order_id_tag,
            clock=clock)

    cpdef void change_guid_factory(self, GuidFactory guid_factory):
        """
        Backtest only method. Change the strategies GUID factory with the given GUID factory.
        
        :param guid_factory: The GUID factory to change to.
        """
        self._guid_factory = guid_factory

    cpdef void change_logger(self, Logger logger):
        """
        Backtest only method. Change the strategies logger with the given logger.
        
        :param logger: The logger to change to.
        """
        self.log = LoggerAdapter(f"{self.id.value}", logger)

    cpdef void set_time(self, datetime time):
        """
        Backtest only method. Set the strategies clock time to the given time.
        
        :param time: The time to set the clock to.
        """
        self.clock.set_time(time)

    cpdef dict iterate(self, datetime time):
        """
        Backtest only method. Iterate the strategies clock to the given time and return
        a list of time events generated by any time alerts or timers.
        
        :param time: The time to iterate the clock to.
        :return Dict[TimeEvent, Callable].
        """
        return self.clock.iterate_time(time)
