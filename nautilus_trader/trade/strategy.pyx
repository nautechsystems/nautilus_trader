# -------------------------------------------------------------------------------------------------
# <copyright file="strategy.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport date, datetime, timedelta

from collections import deque
from typing import List, Dict, Deque

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.types cimport ValidString
from nautilus_trader.common.functions cimport format_zulu_datetime
from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_purpose cimport OrderPurpose
from nautilus_trader.model.c_enums.market_position cimport MarketPosition
from nautilus_trader.model.currency cimport ExchangeRateCalculator
from nautilus_trader.model.events cimport Event, OrderRejected, OrderCancelReject
from nautilus_trader.model.identifiers cimport (  # noqa: E211
    Symbol,
    Label,
    TraderId,
    StrategyId,
    OrderId,
    PositionId)
from nautilus_trader.model.generators cimport PositionIdGenerator
from nautilus_trader.model.objects cimport Quantity, Price, Tick, BarType, Bar, Instrument
from nautilus_trader.model.order cimport Order, AtomicOrder, OrderFactory
from nautilus_trader.model.position cimport Position
from nautilus_trader.common.clock cimport Clock, LiveClock
from nautilus_trader.common.logger cimport Logger, LoggerAdapter, EVT, CMD, SENT, RECV
from nautilus_trader.common.guid cimport GuidFactory, LiveGuidFactory
from nautilus_trader.common.functions cimport fast_mean
from nautilus_trader.common.execution cimport ExecutionEngine
from nautilus_trader.common.data cimport DataClient
from nautilus_trader.common.market cimport IndicatorUpdater
from nautilus_trader.model.commands cimport (  # noqa: E211
    AccountInquiry,
    SubmitOrder,
    SubmitAtomicOrder,
    ModifyOrder,
    CancelOrder)


cdef class TradingStrategy:
    """
    The base class for all trading strategies.
    """

    def __init__(self,
                 str order_id_tag not None='000',
                 bint flatten_on_stop=True,
                 bint flatten_on_sl_reject=True,
                 bint cancel_all_orders_on_stop=True,
                 int tick_capacity=1000,
                 int bar_capacity=1000,
                 Clock clock not None=LiveClock(),
                 GuidFactory guid_factory not None=LiveGuidFactory(),
                 Logger logger=None,
                 bint propagate_exceptions=True):
        """
        Initializes a new instance of the TradingStrategy class.

        :param order_id_tag: The order_id tag for the strategy (must be unique at trader level).
        :param flatten_on_stop: If all strategy positions should be flattened on stop.
        :param flatten_on_sl_reject: If open positions should be flattened on SL reject.
        :param cancel_all_orders_on_stop: If all residual orders should be cancelled on stop.
        :param bar_capacity: The capacity for the internal bar deque(s).
        :param clock: The clock for the strategy.
        :param guid_factory: The GUID factory for the strategy.
        :param logger: The logger for the strategy (can be None).
        :param propagate_exceptions: If exceptions thrown in handling methods should be re-raised.
        :raises ValueError: If the order_id_tag is not a valid string.
        :raises ValueError: If the tick_capacity is not positive (> 0).
        :raises ValueError: If the bar_capacity is not positive (> 0).
        """
        Condition.valid_string(order_id_tag, 'order_id_tag')
        Condition.positive_int(tick_capacity, 'tick_capacity')
        Condition.positive_int(bar_capacity, 'bar_capacity')

        # Identification
        self.id = StrategyId(self.__class__.__name__, order_id_tag)
        self.trader_id = TraderId('TEST', '000')

        # Components
        self.clock = clock
        self.guid_factory = guid_factory
        self.log = LoggerAdapter(self.id.value, logger)

        self.clock.register_logger(self.log)
        self.clock.register_default_handler(self.handle_event)

        # Management flags
        self.flatten_on_stop = flatten_on_stop
        self.flatten_on_sl_reject = flatten_on_sl_reject
        self.cancel_all_orders_on_stop = cancel_all_orders_on_stop
        self.propagate_exceptions = propagate_exceptions

        # Order / Position components
        self.order_factory = OrderFactory(
            id_tag_trader=self.trader_id.order_id_tag,
            id_tag_strategy=self.id.order_id_tag,
            clock=self.clock,
            guid_factory=self.guid_factory)
        self.position_id_generator = PositionIdGenerator(
            id_tag_trader=self.trader_id.order_id_tag,
            id_tag_strategy=self.id.order_id_tag,
            clock=self.clock)

        # Data
        self.tick_capacity = tick_capacity
        self.bar_capacity = bar_capacity
        self._ticks = {}                        # type: Dict[Symbol, Deque[Tick]]
        self._bars = {}                         # type: Dict[BarType, Deque[Bar]]
        self._spreads = {}                      # type: Dict[Symbol, List[float]]
        self._indicators = []                   # type: List[object]
        self._indicator_updaters = {}           # type: Dict[object, List[IndicatorUpdater]]
        self._state_log = []                    # type: List[(datetime, str)]
        self._exchange_calculator = ExchangeRateCalculator()

        # Registerable modules
        self._data_client = None                # Initialized when registered with the data client
        self._exec_engine = None                # Initialized when registered with the execution engine

        self.is_running = False

    cpdef bint equals(self, TradingStrategy other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.id.equals(other.id)

    def __eq__(self, TradingStrategy other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.equals(other)

    def __ne__(self, TradingStrategy other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        :param other: The other object.
        :return bool.
        """
        return not self.equals(other)

    def __hash__(self) -> int:
        """"
        Return the hash code of this object.

        :return int.
        """
        return hash(self.id.value)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return f"{self.__class__.__name__}({self.id.value})"

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{str(self)} object at {id(self)}>"


# -- ABSTRACT METHODS ------------------------------------------------------------------------------

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

    cpdef void on_instrument(self, Instrument instrument) except *:
        """
        Called when an instrument update is received by the strategy.

        :param instrument: The instrument received.
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

    cpdef dict on_save(self):
        """
        Called when the strategy is saved. 'StateLog', 'OrderIdCount' and 'PositionIdCount' are reserved keys.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method on_save() must be implemented in the strategy (or just return empty dictionary).")

    cpdef void on_load(self, dict state) except *:
        """
        Called when the strategy is loaded.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method on_load() must be implemented in the strategy (or just add pass).")

    cpdef void on_dispose(self) except *:
        """
        Called when the strategy is disposed.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method on_dispose() must be implemented in the strategy (or just add pass).")


# -- REGISTRATION METHODS --------------------------------------------------------------------------

    cpdef void register_trader(self, TraderId trader_id) except *:
        """
        Change the trader for the strategy.

        :param trader_id: The trader_id to change to.
        """
        Condition.not_none(trader_id, 'trader_id')

        self.trader_id = trader_id
        self.log.debug(f"Registered trader {trader_id.value}.")

    cpdef void register_data_client(self, DataClient client) except *:
        """
        Register the strategy with the given data client.

        :param client: The data client to register.
        """
        Condition.not_none(client, 'client')

        self._data_client = client
        self.log.debug("Registered data client.")

    cpdef void register_execution_engine(self, ExecutionEngine engine) except *:
        """
        Register the strategy with the given execution engine.

        :param engine: The execution engine to register.
        """
        Condition.not_none(engine, 'engine')

        self._exec_engine = engine
        self.log.debug("Registered execution engine.")
        self.update_state_log(self.time_now(), 'INITIALIZED')

    cpdef void register_indicator(
            self,
            data_source,
            indicator,
            update_method=None) except *:
        """
        Register the given indicator with the strategy to receive data of the
        given data_source (can be Symbol for ticks or BarType).

        :param data_source: The data source for updates.
        :param indicator: The indicator to register.
        :param update_method: The update method for the indicator.
        :raises ValueError: If the update_method is not of type Callable.
        """
        Condition.not_none(data_source, 'data_source')
        Condition.not_none(indicator, 'indicator')
        if update_method is None:
            update_method = indicator.update
        Condition.callable(update_method, 'update_method')

        if indicator not in self._indicators:
            self._indicators.append(indicator)

        if data_source not in self._indicator_updaters:
            self._indicator_updaters[data_source] = []  # type: List[IndicatorUpdater]

        if indicator not in self._indicator_updaters[data_source]:
            self._indicator_updaters[data_source].append(IndicatorUpdater(indicator, update_method))
        else:
            self.log.error(f"Indicator {indicator} already registered for {data_source}.")


# -- HANDLER METHODS -------------------------------------------------------------------------------

    cpdef void handle_tick(self, Tick tick) except *:
        """"
        System method. Update the internal ticks with the given tick, update
        indicators for the symbol, then call on_tick() and pass the tick 
        (if the strategy is running).

        :param tick: The tick received.
        """
        Condition.not_none(tick, 'tick')

        try:
            self._ticks[tick.symbol].appendleft(tick)
            self._spreads[tick.symbol].appendleft(tick.ask.as_double() - tick.bid.as_double())
        except KeyError as ex:  # Symbol was not found
            self._ticks[tick.symbol] = deque([tick], maxlen=self.tick_capacity)
            spread = tick.ask.as_double() - tick.bid.as_double()
            self._spreads[tick.symbol] = deque([spread], maxlen=self.tick_capacity)
            self.log.warning(f"Received {repr(tick)} when not registered. "
                             f"Handling now setup.")

        cdef list updaters = self._indicator_updaters.get(tick.symbol)
        cdef IndicatorUpdater updater
        if updaters is not None:
            for updater in updaters:
                updater.update_tick(tick)

        if self.is_running:
            try:
                self.on_tick(tick)
            except Exception as ex:
                self.log.exception(ex)
                if self.propagate_exceptions:
                    raise ex  # Re-raise

    cpdef void handle_ticks(self, list ticks) except *:
        """
        System method. Handle the given list of ticks by handling each tick individually.
        """
        Condition.not_none(ticks, 'ticks')  # Can be empty

        cdef int length = len(ticks)
        cdef str symbol = ticks[0].symbol.to_string() if length > 0 else '?'
        if length > 0:
            self.log.info(f"Received tick data for {symbol} of {length} ticks.")

        cdef int i
        for i in range(len(ticks)):
            self.handle_tick(ticks[i])

    cpdef void handle_bar(self, BarType bar_type, Bar bar) except *:
        """"
        System method. Update the internal bars with the given bar, update 
        indicators for the bar type, then call on_bar() and pass the arguments 
        (if the strategy is running).

        :param bar_type: The bar type received.
        :param bar: The bar received.
        """
        Condition.not_none(bar_type, 'bar_type')
        Condition.not_none(bar, 'bar')

        try:
            self._bars[bar_type].appendleft(bar)
        except KeyError as ex:
            self._bars[bar_type] = deque([bar], maxlen=self.bar_capacity)
            self.log.warning(f"Received {bar_type} {repr(bar)} when not registered. "
                             f"Handling now setup.")

        cdef list updaters = self._indicator_updaters.get(bar_type)
        cdef IndicatorUpdater updater
        if updaters is not None:
            for updater in updaters:
                updater.update_bar(bar)

        if self.is_running:
            try:
                self.on_bar(bar_type, bar)
            except Exception as ex:
                self.log.exception(ex)
                if self.propagate_exceptions:
                    raise ex  # Re-raise

    cpdef void handle_bars(self, BarType bar_type, list bars) except *:
        """
        System method. Handle the given bar type and bars by handling 
        each bar individually.
        """
        Condition.not_none(bar_type, 'bar_type')
        Condition.not_none(bars, 'bars')  # Can be empty

        self.log.info(f"Received bar data for {bar_type} of {len(bars)} bars.")

        cdef int i
        for i in range(len(bars)):
            self.handle_bar(bar_type, bars[i])

    cpdef void handle_instrument(self, Instrument instrument) except *:
        """
        System method. Handle the given instrument.
        """
        Condition.not_none(instrument, 'instrument')

        if self.is_running:
            try:
                self.on_instrument(instrument)
            except Exception as ex:
                self.log.exception(ex)
                if self.propagate_exceptions:
                    raise ex  # Re-raise

    cpdef void handle_event(self, Event event) except *:
        """
        Call on_event() and passes the event (if the strategy is running).

        :param event: The event received.
        """
        Condition.not_none(event, 'event')

        cdef Order order
        if isinstance(event, OrderRejected):
            self.log.warning(f"{RECV}{EVT} {event}.")
            if self.flatten_on_sl_reject:
                self._flatten_on_sl_reject(event)
        elif isinstance(event, OrderCancelReject):
            self.log.warning(f"{RECV}{EVT} {event}.")
        else:
            self.log.info(f"{RECV}{EVT} {event}.")

        if self.is_running:
            try:
                self.on_event(event)
            except Exception as ex:
                self.log.exception(ex)
                if self.propagate_exceptions:
                    raise ex  # Re-raise


# -- DATA METHODS ----------------------------------------------------------------------------------

    cpdef datetime time_now(self):
        """
        Return the current time from the strategies internal clock (UTC).
        
        :return datetime.
        """
        return self.clock.time_now()

    cpdef list instrument_symbols(self):
        """
        Return a list of all instrument symbols held by the data client.
        
        :return List[Instrument].
        :raises ValueError: If the strategy has not been registered with a data client.
        """
        Condition.not_none(self._data_client, 'data_client')

        return self._data_client.instrument_symbols()

    cpdef void get_ticks(
            self,
            Symbol symbol,
            date from_date=None,
            date to_date=None,
            int limit=0) except *:
        """
        Request the historical bars for the given parameters from the data service.
        Note: Logs warning if the downloaded bars 'from' datetime is greater than that given.

        :param symbol: The symbol for the request.
        :param from_date: The from date for the request.
        :param to_date: The to date for the request.
        :param limit: The limit for the number of ticks in the response (default = tick capacity).
        :raises ValueError: If the limit is negative (< 0).
        :raises ValueError: If the from_datetime is not None and not less than to_datetime.
        """
        if limit == 0:
            limit = self.tick_capacity
        Condition.not_none(symbol, 'symbol')
        Condition.not_negative_int(limit, 'limit')

        if self._data_client is None:
            self.log.error("Cannot request ticks (data client not registered).")
            return

        if to_date is None:
            to_date = self.clock.date_now()
        if from_date is None:
            from_date = self.clock.date_now() - timedelta(days=1)

        Condition.true(from_date < to_date, 'from_datetime < to_date')

        if self._data_client is None:
            self.log.error("Cannot download historical bars (data client not registered).")
            return

        if symbol not in self._ticks:
            self._ticks[symbol] = deque(maxlen=self.tick_capacity)

        if symbol not in self._spreads:
            self._spreads[symbol] = deque(maxlen=self.tick_capacity)

        self._data_client.request_ticks(
            symbol,
            from_date,
            to_date,
            limit,
            self.handle_ticks)

    cpdef void get_bars(
            self,
            BarType bar_type,
            date from_date=None,
            date to_date=None,
            int limit=0) except *:
        """
        Request the historical bars for the given parameters from the data service.
        Note: Logs warning if the downloaded bars 'from' datetime is greater than that given.

        :param bar_type: The historical bar type to download.
        :param from_date: The from date for the request.
        :param to_date: The to date for the request.
        :param limit: The limit for the number of bars in the response (default = bar capacity).
        :raises ValueError: If the limit is negative (< 0).
        :raises ValueError: If the from_datetime is not None and not less than to_datetime.
        """
        if limit == 0:
            limit = self.bar_capacity
        Condition.not_none(bar_type, 'bar_type')
        Condition.not_negative_int(limit, 'limit')

        if self._data_client is None:
            self.log.error("Cannot request bars (data client not registered).")
            return

        if to_date is None:
            to_date = self.clock.date_now()
        if from_date is None:
            from_date = self.clock.date_now() - timedelta(days=1)

        Condition.true(from_date < to_date, 'from_datetime < to_date')

        if bar_type not in self._bars:
            self._bars[bar_type] = deque(maxlen=self.bar_capacity)

        if self._data_client is None:
            self.log.error("Cannot download historical bars (data client not registered).")
            return

        self._data_client.request_bars(
            bar_type,
            from_date,
            to_date,
            limit,
            self.handle_bars)

    cpdef Instrument get_instrument(self, Symbol symbol):
        """
        Return the instrument corresponding to the given symbol (if found).

        :param symbol: The symbol of the instrument to return.
        :return Instrument or None.
        :raises ValueError: If strategy has not been registered with a data client.
        """
        if self._data_client is None:
            self.log.error("Cannot get instrument (data client not registered).")
            return

        return self._data_client.get_instrument(symbol)

    cpdef dict get_instruments(self):
        """
        Return a dictionary of all instruments for the given venue (if found).
        
        :return Dict[Symbol, Instrument].
        """
        if self._data_client is None:
            self.log.error("Cannot get instruments (data client not registered).")
            return

        return self._data_client.get_instruments()

    cpdef void subscribe_ticks(self, Symbol symbol) except *:
        """
        Subscribe to tick data for the given symbol.

        :param symbol: The tick symbol to subscribe to.
        """
        Condition.not_none(symbol, 'symbol')

        if self._data_client is None:
            self.log.error("Cannot subscribe to ticks (data client not registered).")
            return

        if symbol not in self._ticks:
            self._ticks[symbol] = deque(maxlen=self.tick_capacity)

        if symbol not in self._spreads:
            self._spreads[symbol] = deque(maxlen=self.tick_capacity)

        self._data_client.subscribe_ticks(symbol, self.handle_tick)
        self.log.info(f"Subscribed to {symbol} tick data.")

    cpdef void subscribe_bars(self, BarType bar_type) except *:
        """
        Subscribe to bar data for the given bar type.

        :param bar_type: The bar type to subscribe to.
        """
        Condition.not_none(bar_type, 'bar_type')

        if self._data_client is None:
            self.log.error("Cannot subscribe to bars (data client not registered).")
            return

        if bar_type not in self._bars:
            self._bars[bar_type] = deque(maxlen=self.bar_capacity)

        self._data_client.subscribe_bars(bar_type, self.handle_bar)
        self.log.info(f"Subscribed to {bar_type} bar data.")

    cpdef void subscribe_instrument(self, Symbol symbol) except *:
        """
        Subscribe to instrument data for the given symbol.

        :param symbol: The instrument symbol to subscribe to.
        """
        Condition.not_none(symbol, 'symbol')

        if self._data_client is None:
            self.log.error("Cannot subscribe to instrument (data client not registered).")
            return

        self._data_client.subscribe_instrument(symbol, self.handle_instrument)
        self.log.info(f"Subscribed to {symbol} instrument data.")

    cpdef void unsubscribe_ticks(self, Symbol symbol) except *:
        """
        Unsubscribe from tick data for the given symbol.

        :param symbol: The tick symbol to unsubscribe from.
        """
        Condition.not_none(symbol, 'symbol')

        if self._data_client is None:
            self.log.error("Cannot unsubscribe from ticks (data client not registered).")
            return

        self._data_client.unsubscribe_ticks(symbol, self.handle_tick)
        self.log.info(f"Unsubscribed from {symbol} tick data.")

    cpdef void unsubscribe_bars(self, BarType bar_type) except *:
        """
        Unsubscribe from bar data for the given bar type.

        :param bar_type: The bar type to unsubscribe from.
        """
        Condition.not_none(bar_type, 'bar_type')

        if self._data_client is None:
            self.log.error("Cannot unsubscribe from bars (data client not registered).")
            return

        self._data_client.unsubscribe_bars(bar_type, self.handle_bar)
        self.log.info(f"Unsubscribed from {bar_type} bar data.")

    cpdef void unsubscribe_instrument(self, Symbol symbol) except *:
        """
        Unsubscribe from instrument data for the given symbol.

        :param symbol: The instrument symbol to unsubscribe from.
        """
        Condition.not_none(symbol, 'symbol')

        if self._data_client is None:
            self.log.error("Cannot unsubscribe from instrument (data client not registered).")
            return

        self._data_client.unsubscribe_instrument(symbol, self.handle_instrument)
        self.log.info(f"Unsubscribed from {symbol} instrument data.")

    cpdef bint has_ticks(self, Symbol symbol):
        """
        Return a value indicating whether the strategy has ticks for the given symbol.
        
        :param symbol: The symbol of the ticks.
        :return bool.
        """
        Condition.not_none(symbol, 'symbol')

        return symbol in self._ticks and len(self._ticks[symbol]) > 0

    cpdef bint has_bars(self, BarType bar_type):
        """
        Return a value indicating whether the strategy has bars for the given bar type.
        
        :param bar_type: The bar_type of the bars.
        :return bool.
        """
        Condition.not_none(bar_type, 'bar_type')

        return bar_type in self._bars and len(self._bars[bar_type]) > 0

    cpdef int tick_count(self, Symbol symbol):
        """
        Return the count of ticks held by the strategy for the given symbol.
        
        :param symbol: The tick symbol to count.
        :return int.
        """
        Condition.not_none(symbol, 'symbol')

        return len(self._ticks[symbol]) if symbol in self._ticks else 0

    cpdef int bar_count(self, BarType bar_type):
        """
        Return the count of ticks held by the strategy for the given symbol.
        
        :param bar_type: The bar type to count.
        :return int.
        """
        Condition.not_none(bar_type, 'bar_type')

        return len(self._bars[bar_type]) if bar_type in self._bars else 0

    cpdef list ticks(self, Symbol symbol):
        """
        Return the ticks for the given symbol (returns a copy of the internal deque).

        :param symbol: The symbol for the ticks to get.
        :return List[Tick].
        """
        Condition.not_none(symbol, 'symbol')
        Condition.is_in(symbol, self._ticks, 'symbol', 'ticks')

        return list(self._ticks[symbol])

    cpdef list bars(self, BarType bar_type):
        """
        Return the bars for the given bar type (returns a copy of the internal deque).

        :param bar_type: The bar type to get.
        :return List[Bar].
        """
        Condition.not_none(bar_type, 'bar_type')
        Condition.is_in(bar_type, self._bars, 'bar_type', 'bars')

        return list(self._bars[bar_type])

    cpdef Tick tick(self, Symbol symbol, int index=0):
        """
        Return the tick for the given symbol at the given index or last if no index specified.

        :param symbol: The symbol for the tick to get.
        :param index: The optional index for the tick to get .
        :return Tick.
        :raises ValueError: If the strategies ticks does not contain the symbol.
        :raises IndexError: If the tick index is out of range.
        """
        Condition.not_none(symbol, 'symbol')
        Condition.is_in(symbol, self._ticks, 'symbol', 'ticks')

        return self._ticks[symbol][index]

    cpdef Bar bar(self, BarType bar_type, int index=0):
        """
        Return the bar for the given bar type at the given index or last if no index specified.

        :param bar_type: The bar type to get.
        :param index: The optional index for the bar to get.
        :return Bar.
        :raises ValueError: If the strategies bars does not contain the bar type.
        :raises IndexError: If the bar index is out of range.
        """
        Condition.not_none(bar_type, 'bar_type')
        Condition.is_in(bar_type, self._bars, 'bar_type', 'bars')

        return self._bars[bar_type][index]

    cpdef double spread(self, Symbol symbol):
        """
        Return the current spread for the given symbol.
        
        :param symbol: The symbol for the spread to get.
        :return float.
        :raises ValueError: If the strategies ticks does not contain the symbol.
        """
        Condition.not_none(symbol, 'symbol')
        Condition.is_in(symbol, self._spreads, 'symbol', 'spreads')

        return self._spreads[symbol][0]

    cpdef double average_spread(self, Symbol symbol, int window=0):
        """
        Return the average spread over the look back window.
        
        :param symbol: The symbol for the average spread to get.
        :param window: The optional custom window for the average (> 0).
        :return float.
        :raises ValueError: If the strategies ticks does not contain the symbol.
        """
        if window == 0:
            window = self.tick_capacity
        Condition.not_none(symbol, 'symbol')
        Condition.is_in(symbol, self._spreads, 'symbol', 'ticks')
        Condition.positive_int(window, 'window')

        cdef list spreads = list(self._spreads[symbol])[:window]
        return fast_mean(spreads)


# -- INDICATOR METHODS -----------------------------------------------------------------------------

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
        cdef int i
        for i in range(len(self._indicators)):
            if self._indicators[i].initialized is False:
                return False
        return True


# -- MANAGEMENT METHODS ----------------------------------------------------------------------------

    cpdef Account account(self):
        """
        Return the account for the strategy.
        
        :return: Account.
        :raises: ValueError: If the execution engine is not registered.
        """
        Condition.not_none(self._exec_engine, 'is_execution_engine_registered')

        return self._exec_engine.account

    cpdef Portfolio portfolio(self):
        """
        Return the portfolio for the strategy.
        
        :return: Portfolio.
        :raises: ValueError: If the execution engine is not registered.
        """
        Condition.not_none(self._exec_engine, 'is_execution_engine_registered')

        return self._exec_engine.portfolio

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
        :raises ValueError: If the given market position is FLAT.
        """
        if market_position == MarketPosition.LONG:
            return OrderSide.SELL
        elif market_position == MarketPosition.SHORT:
            return OrderSide.BUY
        else:
            raise ValueError("Cannot flatten a FLAT position.")

    cpdef double get_exchange_rate(
            self,
            Currency from_currency,
            Currency to_currency,
            PriceType price_type=PriceType.MID):
        """
        Return the calculated exchange rate for the given currencies.

        :param from_currency: The currency to convert from.
        :param to_currency: The currency to convert to.
        :param price_type: The quote type for the exchange rate (default=MID).
        :return float.
        :raises ValueError: If the quote type is LAST.
        """
        cdef Account account = self.account()
        if account is None:
            self.log.error("Cannot get exchange rate (account is not initialized).")
            return 0.0

        cdef Symbol symbol
        cdef dict bid_rates = {symbol.code: ticks[0].bid.as_double() for symbol, ticks in self._ticks.items()}
        cdef dict ask_rates = {symbol.code: ticks[0].ask.as_double() for symbol, ticks in self._ticks.items()}

        return self._exchange_calculator.get_rate(
            from_currency=from_currency,
            to_currency=to_currency,
            price_type=price_type,
            bid_rates=bid_rates,
            ask_rates=ask_rates)

    cpdef double get_exchange_rate_for_account(
            self,
            Currency quote_currency,
            PriceType price_type=PriceType.MID):
        """
        Return the calculated exchange rate for the give trading instrument quote 
        currency to the account currency.

        :param quote_currency: The quote currency to convert from.
        :param price_type: The quote type for the exchange rate (default=MID).
        :return float.
        :raises ValueError: If the quote type is LAST.
        """
        cdef Account account = self.account()
        if account is None:
            self.log.error("Cannot get exchange rate (account is not initialized).")
            return 0.0

        cdef Symbol symbol
        cdef dict bid_rates = {symbol.code: ticks[0].bid.as_double() for symbol, ticks in self._ticks.items()}
        cdef dict ask_rates = {symbol.code: ticks[0].ask.as_double() for symbol, ticks in self._ticks.items()}

        return self._exchange_calculator.get_rate(
            from_currency=quote_currency,
            to_currency=self.account().currency,
            price_type=price_type,
            bid_rates=bid_rates,
            ask_rates=ask_rates)

    cpdef Order order(self, OrderId order_id):
        """
        Return the order with the given identifier (if found).

        :param order_id: The order_id.
        :return Order or None.
        """
        return self._exec_engine.database.get_order(order_id)

    cpdef dict orders(self):
        """
        Return a all orders associated with this strategy.
        
        :return Dict[OrderId, Order].
        """
        return self._exec_engine.database.get_orders(self.id)

    cpdef dict orders_working(self):
        """
        Return all working orders associated with this strategy.
        
        :return Dict[OrderId, Order].
        """
        return self._exec_engine.database.get_orders_working(self.id)

    cpdef dict orders_completed(self):
        """
        Return all completed orders associated with this strategy.
        
        :return Dict[OrderId, Order].
        """
        return self._exec_engine.database.get_orders_completed(self.id)

    cpdef Position position(self, PositionId position_id):
        """
        Return the position associated with the given position_id (if found).

        :param position_id: The positions identifier.
        :return Position or None.
        """
        return self._exec_engine.database.get_position(position_id)

    cpdef Position position_for_order(self, OrderId order_id):
        """
        Return the position associated with the given order_id (if found).

        :param order_id: The order_id.
        :return Position or None.
        """
        return self._exec_engine.database.get_position_for_order(order_id)

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

    cpdef bint is_order_completed(self, OrderId order_id):
        """
        Return a value indicating whether an order with the given identifier is complete.
         
        :param order_id: The order_id.
        :return bool.
        """
        return self._exec_engine.database.is_order_completed(order_id)

    cpdef bint is_position_open(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given identifier is open.
         
        :param position_id: The position_id.
        :return bool.
        """
        return self._exec_engine.database.is_position_open(position_id)

    cpdef bint is_position_closed(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given identifier is closed.
         
        :param position_id: The position_id.
        :return bool.
        """
        return self._exec_engine.database.is_position_closed(position_id)

    cpdef bint is_flat(self):
        """
        Return a value indicating whether the strategy is completely flat (i.e no market positions
        other than FLAT across all instruments).
        
        :return bool.
        """
        return self._exec_engine.is_strategy_flat(self.id)

    cpdef int count_orders_working(self):
        """
        Return the count of working orders held by the execution database.
        
        :return int.
        """
        return self._exec_engine.database.count_orders_working(self.id)

    cpdef int count_orders_completed(self):
        """
        Return the count of completed orders held by the execution database.
        
        :return int.
        """
        return self._exec_engine.database.count_orders_completed(self.id)

    cpdef int count_orders_total(self):
        """
        Return the total count of orders held by the execution database.
        
        :return int.
        """
        return self._exec_engine.database.count_orders_total(self.id)

    cpdef int count_positions_open(self):
        """
        Return the count of open positions held by the execution database.
        
        :return int.
        """
        return self._exec_engine.database.count_positions_open(self.id)

    cpdef int count_positions_closed(self):
        """
        Return the count of closed positions held by the execution database.
        
        :return int.
        """
        return self._exec_engine.database.count_positions_closed(self.id)

    cpdef int count_positions_total(self):
        """
        Return the total count of positions held by the execution database.
        
        :return int.
        """
        return self._exec_engine.database.count_positions_total(self.id)


# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void start(self) except *:
        """
        Start the trade strategy and call on_start().
        """
        self.update_state_log(self.time_now(), 'STARTING')
        self.log.debug(f"Starting...")

        if self._data_client is None:
            self.log.error("Cannot start strategy (the data client is not registered).")
            return

        if self._exec_engine is None:
            self.log.error("Cannot start strategy (the execution engine is not registered).")
            return

        try:
            self.on_start()
        except Exception as ex:
            self.log.exception(ex)

        self.is_running = True
        self.update_state_log(self.time_now(), 'RUNNING')
        self.log.info(f"Running...")

    cpdef void stop(self) except *:
        """
        Stop the trade strategy and call on_stop().
        """
        self.update_state_log(self.time_now(), 'STOPPING')
        self.log.debug(f"Stopping...")

        # Clean up clock
        self.clock.cancel_all_timers()

        # Flatten open positions
        if self.flatten_on_stop:
            if not self.is_flat():
                self.flatten_all_positions()

        # Cancel working orders
        if self.cancel_all_orders_on_stop:
            self.cancel_all_orders("STOPPING STRATEGY")

        try:
            self.on_stop()
        except Exception as ex:
            self.log.exception(ex)

        self.is_running = False
        self.update_state_log(self.time_now(), 'STOPPED')
        self.log.info(f"Stopped.")

    cpdef void reset(self) except *:
        """
        Reset the strategy by returning all stateful values to their
        initial value, the on_reset() implementation is then called. 
        Note: The strategy cannot be running otherwise an error is logged.
        """
        if self.is_running:
            self.log.error(f"Cannot reset (cannot reset a running strategy).")
            return

        self.log.debug(f"Resetting...")

        self.order_factory.reset()
        self.position_id_generator.reset()
        self._ticks.clear()
        self._bars.clear()
        self._spreads.clear()
        self._indicators.clear()
        self._indicator_updaters.clear()
        self._state_log.clear()

        for indicator in self._indicators:
            indicator.reset()

        try:
            self.on_reset()
        except Exception as ex:
            self.log.exception(ex)

        self.log.info(f"Reset.")

    cpdef dict save(self):
        """
        Return the strategy state dictionary to be saved.
        """
        self.update_state_log(self.time_now(), 'SAVING...')

        cpdef dict state = {
            'StateLog': self._state_log,
            'OrderIdCount': self.order_factory.count(),
            'PositionIdCount': self.position_id_generator.count
        }

        try:
            user_state = self.on_save()
        except Exception as ex:
            self.log.exception(ex)

        return {**state, **user_state}

    cpdef void saved(self, datetime timestamp) except *:
        """
        System Method: Add a SAVED state to the state log.
        
        :param timestamp: The timestamp when the strategy was saved.
        """
        Condition.not_none(timestamp, 'timestamp')

        self.update_state_log(timestamp, 'SAVED')

    cpdef void load(self, dict state) except *:
        """
        Load the strategy state from the give state dictionary.
        
        :param state: The state dictionary to load.
        """
        Condition.not_empty(state, 'state')

        state_log = state.get(b'StateLog')
        cdef list buffered = self._state_log
        if state_log:
            self._state_log = []
            for value in state_log:
                self._state_log.append(value.decode('utf8'))
            self._state_log.extend(buffered)

        self.update_state_log(self.time_now(), 'LOADING...')

        order_id_count = state.get(b'OrderIdCount')
        if order_id_count:
            order_id_count = int(order_id_count.decode('utf8'))
            self.order_factory.set_count(order_id_count)
            self.log.info(f"Setting OrderIdGenerator count to {order_id_count}.")

        position_id_count = state.get(b'PositionIdCount')
        if position_id_count:
            position_id_count = int(position_id_count.decode('utf8'))
            self.position_id_generator.set_count(position_id_count)
            self.log.info(f"Setting PositionIdGenerator count to {position_id_count}.")

        try:
            self.on_load(state)
        except Exception as ex:
            self.log.exception(ex)

        self.update_state_log(self.time_now(), 'LOADED')

    cpdef void dispose(self) except *:
        """
        Dispose of the strategy to release system resources, then call on_dispose().
        """
        self.log.debug(f"Disposing...")

        try:
            self.on_dispose()
        except Exception as ex:
            self.log.exception(ex)

        self.log.info(f"Disposed.")

    cpdef void update_state_log(self, datetime timestamp, str action) except *:
        Condition.not_none(timestamp, 'timestamp')
        Condition.valid_string(action, 'action')

        self._state_log.append(f'{format_zulu_datetime(timestamp)} {action}')

    cpdef void account_inquiry(self) except *:
        """
        Send an account inquiry command to the execution service.
        """
        if self._exec_engine is None:
            self.log.error("Cannot send command AccountInquiry (execution engine not registered).")
            return

        cdef AccountInquiry command = AccountInquiry(
            self.trader_id,
            self._exec_engine.account_id,
            self.guid_factory.generate(),
            self.clock.time_now())

        self.log.info(f"{CMD}{SENT} {command}.")
        self._exec_engine.execute_command(command)

    cpdef void submit_order(self, Order order, PositionId position_id) except *:
        """
        Send a submit order command with the given order and position_id to the execution 
        service.

        :param order: The order to submit.
        :param position_id: The position_id to associate with this order.
        """
        Condition.not_none(order, 'order')
        Condition.not_none(position_id, 'position_id')

        if self._exec_engine is None:
            self.log.error("Cannot send command SubmitOrder (execution engine not registered).")
            return

        cdef SubmitOrder command = SubmitOrder(
            self.trader_id,
            self._exec_engine.account_id,
            self.id,
            position_id,
            order,
            self.guid_factory.generate(),
            self.clock.time_now())

        self.log.info(f"{CMD}{SENT} {command}.")
        self._exec_engine.execute_command(command)

    cpdef void submit_atomic_order(self, AtomicOrder atomic_order, PositionId position_id) except *:
        """
        Send a submit atomic order command with the given order and position_id to the 
        execution service.
        
        :param atomic_order: The atomic order to submit.
        :param position_id: The position_id to associate with this order.
        """
        Condition.not_none(atomic_order, 'atomic_order')
        Condition.not_none(position_id, 'position_id')

        if self._exec_engine is None:
            self.log.error("Cannot command SubmitAtomicOrder (execution engine not registered).")
            return

        cdef SubmitAtomicOrder command = SubmitAtomicOrder(
            self.trader_id,
            self._exec_engine.account_id,
            self.id,
            position_id,
            atomic_order,
            self.guid_factory.generate(),
            self.clock.time_now())

        self.log.info(f"{CMD}{SENT} {command}.")
        self._exec_engine.execute_command(command)

    cpdef void modify_order(self, Order order, Quantity new_quantity=None, Price new_price=None) except *:
        """
        Send a modify order command for the given order with the given new price
        to the execution service.

        :param order: The order to modify.
        :param new_quantity: The new quantity for the given order.
        :param new_price: The new price for the given order.
        """
        Condition.not_none(order, 'order')

        if self._exec_engine is None:
            self.log.error("Cannot send command ModifyOrder (execution engine not registered).")
            return

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
            order.id,
            new_quantity,
            new_price,
            self.guid_factory.generate(),
            self.clock.time_now())

        self.log.info(f"{CMD}{SENT} {command}.")
        self._exec_engine.execute_command(command)

    cpdef void cancel_order(self, Order order, str cancel_reason='NONE') except *:
        """
        Send a cancel order command for the given order and cancel_reason to the
        execution service.

        :param order: The order to cancel.
        :param cancel_reason: The reason for cancellation (will be logged).
        :raises ValueError: If the strategy has not been registered with an execution client.
        :raises ValueError: If the cancel_reason is not a valid string.
        """
        Condition.not_none(order, 'order')
        Condition.not_none(cancel_reason, 'cancel_reason')  # Can be empty string

        if self._exec_engine is None:
            self.log.error("Cannot send command CancelOrder (execution client not registered).")
            return

        cdef CancelOrder command = CancelOrder(
            self.trader_id,
            self._exec_engine.account_id,
            order.id,
            ValidString(cancel_reason),
            self.guid_factory.generate(),
            self.clock.time_now())

        self.log.info(f"{CMD}{SENT} {command}.")
        self._exec_engine.execute_command(command)

    cpdef void cancel_all_orders(self, str cancel_reason='CANCEL_ON_STOP') except *:
        """
        Send a cancel order command for orders which are not completed in the
        order book with the given cancel_reason - to the execution engine.

        :param cancel_reason: The reason for cancellation (default='NONE').
        :raises ValueError: If the cancel_reason is not a valid string.
        """
        Condition.not_none(cancel_reason, 'cancel_reason')  # Can be empty string

        if self._exec_engine is None:
            self.log.error("Cannot execute CANCEL_ALL_ORDERS, execution client not registered.")
            return

        cdef dict working_orders = self._exec_engine.database.get_orders_working(self.id)
        cdef int working_orders_count = len(working_orders)
        if working_orders_count == 0:
            self.log.info("CANCEL_ALL_ORDERS: No working orders to cancel.")
            return

        self.log.info(f"CANCEL_ALL_ORDERS: Cancelling {working_orders_count} working order(s)...")
        cdef OrderId order_id
        cdef Order order
        cdef CancelOrder command
        for order_id, order in working_orders.items():
            command = CancelOrder(
                self.trader_id,
                self._exec_engine.account_id,
                order_id,
                ValidString(cancel_reason),
                self.guid_factory.generate(),
                self.clock.time_now())

            self.log.info(f"{CMD}{SENT} {command}.")
            self._exec_engine.execute_command(command)

    cpdef void flatten_position(self, PositionId position_id, Label label=Label('FLATTEN')) except *:
        """
        Flatten the position corresponding to the given identifier by generating
        the required market order, and sending it to the execution service.
        If the position is None or already FLAT will log a warning.

        :param position_id: The position_id to flatten.
        :param label: The label for the flattening order.
        :raises ValueError: If the position_id is not found in the position book.
        """
        Condition.not_none(position_id, 'position_id')
        Condition.not_none(label, 'label')

        if self._exec_engine is None:
            self.log.error("Cannot flatten position (execution client not registered).")
            return

        cdef Position position = self._exec_engine.database.get_position(position_id)
        if position is None:
            self.log.error(f"Cannot flatten position (cannot find {position_id} in cached positions.")
            return

        if position.is_closed:
            self.log.warning(f"Cannot flatten position (the position {position_id} was already closed).")
            return

        cdef Order order = self.order_factory.market(
            position.symbol,
            self.get_flatten_side(position.market_position),
            position.quantity,
            label,
            OrderPurpose.EXIT)

        self.log.info(f"Flattening {position}...")
        self.submit_order(order, position_id)

    cpdef void flatten_all_positions(self, Label label=Label('FLATTEN')) except *:
        """
        Flatten all positions by generating the required market orders and sending
        them to the execution service. If no positions found or a position is None
        then will log a warning.
        
        :param label: The label for the flattening order(s).
        """
        Condition.not_none(label, 'label')

        if self._exec_engine is None:
            self.log.error("Cannot flatten all positions (execution client not registered).")
            return

        cdef dict positions = self._exec_engine.database.get_positions_open(self.id)
        cdef int open_positions_count = len(positions)
        if open_positions_count == 0:
            self.log.info("FLATTEN_ALL_POSITIONS: No open positions to flatten.")
            return

        self.log.info(f"FLATTEN_ALL_POSITIONS: Flattening {open_positions_count} open position(s)...")

        cdef PositionId position_id
        cdef Position position
        cdef Order order
        for position_id, position in positions.items():
            if position.is_closed:
                self.log.warning(f"Cannot flatten position (the position {position_id} was already FLAT.")
                continue

            order = self.order_factory.market(
                position.symbol,
                self.get_flatten_side(position.market_position),
                position.quantity,
                label,
                OrderPurpose.EXIT)

            self.log.info(f"Flattening {position}...")
            self.submit_order(order, position_id)

    cdef void _flatten_on_sl_reject(self, OrderRejected event) except *:
        cdef Order order = self._exec_engine.database.get_order(event.order_id)
        cdef PositionId position_id = self._exec_engine.database.get_position_id(event.order_id)

        if order is None:
            self.log.error(f"Cannot find {event.order_id} in cached orders.")
            return

        if position_id is None:
            self.log.error(f"Cannot find PositionId for {event.order_id}.")
            return

        if order.purpose == OrderPurpose.STOP_LOSS:
            if self._exec_engine.database.is_position_open(position_id):
                self.log.error(f"Rejected {event.order_id} was a stop-loss, now flattening {position_id}.")
                self.flatten_position(position_id)


# -- BACKTEST METHODS ------------------------------------------------------------------------------

    cpdef void change_clock(self, Clock clock) except *:
        """
        Backtest only method. Change the strategies clock with the given clock.
        
        :param clock: The clock to change to.
        """
        Condition.not_none(clock, 'clock')

        self.clock = clock
        self.clock.register_logger(self.log)
        self.clock.register_default_handler(self.handle_event)

        self.order_factory = OrderFactory(
            id_tag_trader=self.trader_id.order_id_tag,
            id_tag_strategy=self.id.order_id_tag,
            clock=clock,
            guid_factory=self.guid_factory,
            initial_count=self.order_factory.count())

        self.position_id_generator = PositionIdGenerator(
            id_tag_trader=self.trader_id.order_id_tag,
            id_tag_strategy=self.id.order_id_tag,
            clock=clock,
            initial_count=self.position_id_generator.count)

    cpdef void change_guid_factory(self, GuidFactory guid_factory) except *:
        """
        Backtest only method. Change the strategies GUID factory with the given GUID factory.
        
        :param guid_factory: The GUID factory to change to.
        """
        Condition.not_none(guid_factory, 'guid_factory')

        self.guid_factory = guid_factory

    cpdef void change_logger(self, Logger logger) except *:
        """
        Backtest only method. Change the strategies logger with the given logger.
        
        :param logger: The logger to change to.
        """
        Condition.not_none(logger, 'logger')

        self.log = LoggerAdapter(self.id.value, logger)
