#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="strategy.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

import uuid

from cpython.datetime cimport datetime, timedelta
from collections import deque
from typing import Callable, Dict, List, Deque

from inv_trader.core.precondition cimport Precondition
from inv_trader.enums.order_side cimport OrderSide
from inv_trader.enums.market_position cimport MarketPosition
from inv_trader.common.clock cimport Clock, LiveClock
from inv_trader.common.logger cimport Logger, LoggerAdapter
from inv_trader.common.execution cimport ExecutionClient
from inv_trader.common.data cimport DataClient
from inv_trader.common.guid cimport GuidFactory, LiveGuidFactory
from inv_trader.model.events cimport Event
from inv_trader.model.identifiers cimport GUID, Label, OrderId, PositionId, PositionIdGenerator
from inv_trader.model.objects cimport ValidString, Symbol, Price, Tick, BarType, Bar, Instrument
from inv_trader.model.order cimport Order, AtomicOrder, OrderFactory
from inv_trader.model.position cimport Position
from inv_trader.commands cimport CollateralInquiry, SubmitOrder, SubmitAtomicOrder, ModifyOrder, CancelOrder
from inv_trader.tools cimport IndicatorUpdater
Indicator = object


cdef class TradeStrategy:
    """
    The abstract base class for all trade strategies.
    """

    def __init__(self,
                 str label='001',
                 str id_tag_trader='001',
                 str id_tag_strategy='001',
                 int bar_capacity=1000,
                 Clock clock=LiveClock(),
                 GuidFactory guid_factory=LiveGuidFactory(),
                 Logger logger=None):
        """
        Initializes a new instance of the TradeStrategy abstract class.

        :param label: The unique label for the strategy.
        :param id_tag_trader: The unique order identifier tag for the trader.
        :param id_tag_strategy: The unique order identifier tag for the strategy.
        :param bar_capacity: The capacity for the internal bar deque(s).
        :param clock: The clock for the strategy.
        :param guid_factory: The GUID factory for the strategy.
        :param logger: The logger (can be None, and will print).
        :raises ValueError: If the label is not a valid string.
        :raises ValueError: If the id_tag_trader is not a valid string.
        :raises ValueError: If the order_id_tag is not a valid string.
        :raises ValueError: If the bar_capacity is not positive (> 0).
        :raises ValueError: If the clock is None.
        :raises ValueError: If the guid_factory is None.
        """
        Precondition.positive(bar_capacity, 'bar_capacity')
        Precondition.not_none(clock, 'clock')
        Precondition.not_none(guid_factory, 'guid_factory')

        self._clock = clock
        self._guid_factory = guid_factory
        self.name = Label(self.__class__.__name__ + '-' + label)
        if logger is None:
            self.log = LoggerAdapter(f"{self.name.value}")
        else:
            self.log = LoggerAdapter(f"{self.name.value}", logger)
        self.id_tag_trader = ValidString(id_tag_trader)
        self.id_tag_strategy = ValidString(id_tag_strategy)
        self.id = GUID(uuid.uuid4())
        self.position_id_generator = PositionIdGenerator(
            id_tag_trader=self.id_tag_trader,
            id_tag_strategy=self.id_tag_strategy,
            clock=self._clock)
        self.order_factory = OrderFactory(
            id_tag_trader=self.id_tag_trader,
            id_tag_strategy=self.id_tag_strategy,
            clock=self._clock)
        self.bar_capacity = bar_capacity
        self.is_running = False
        self._ticks = {}               # type: Dict[Symbol, Tick]
        self._bars = {}                # type: Dict[BarType, Deque[Bar]]
        self._indicators = {}          # type: Dict[BarType, List[Indicator]]
        self._indicator_updaters = {}  # type: Dict[BarType, List[IndicatorUpdater]]
        self._data_client = None       # Initialized when registered with data client
        self._exec_client = None       # Initialized when registered with execution client
        self._portfolio = None         # Initialized when registered with execution client
        self.account = None            # Initialized when registered with execution client

        self.log.info(f"Initialized.")

    cdef bint equals(self, TradeStrategy other):
        """
        Compare if the object equals the given object.
        
        :param other: The other object to compare
        :return: True if the objects are equal, otherwise False.
        """
        return self.id.equals(other.id)

    def __eq__(self, other) -> bool:
        """
        Override the default equality comparison.
        """
        return self.equals(other)

    def __ne__(self, other) -> bool:
        """
        Override the default not-equals comparison.
        """
        return not self.equals(other)

    def __hash__(self):
        """"
        Override the default hash implementation.
        """
        return hash(self.name)

    def __str__(self) -> str:
        """
        :return: The str() string representation of the strategy.
        """
        return f"TradeStrategy({self.name.value})"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the strategy.
        """
        return f"<{str(self)} object at {id(self)}>"


# -- ABSTRACT METHODS ---------------------------------------------------------------------------- #

    cpdef void on_start(self):
        """
        Called when the strategy is started.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the strategy (or just add pass).")

    cpdef void on_tick(self, Tick tick):
        """
        Called when a tick is received by the strategy.

        :param tick: The tick received.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the strategy (or just add pass).")

    cpdef void on_bar(self, BarType bar_type, Bar bar):
        """
        Called when a bar is received by the strategy.

        :param bar_type: The bar type received.
        :param bar: The bar received.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the strategy (or just add pass).")

    cpdef void on_event(self, Event event):
        """
        Called when an event is received by the strategy.

        :param event: The event received.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the strategy (or just add pass).")

    cpdef void on_stop(self):
        """
        Called when the strategy is stopped.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the strategy (or just add pass).")

    cpdef void on_reset(self):
        """
        Called when the strategy is reset.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the strategy (or just add pass).")

    cpdef void on_dispose(self):
        """
        Called when the strategy is disposed.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the strategy (or just add pass).")

# -- REGISTRATION AND HANDLER METHODS ------------------------------------------------------------ #

    cpdef void register_data_client(self, DataClient client):
        """
        Register the strategy with the given data client.

        :param client: The data client to register.
        :raises ValueError: If client is None.
        """
        Precondition.not_none(client, 'client')

        self._data_client = client
        self.log.info("Registered data client.")

    cpdef void register_execution_client(self, ExecutionClient client):
        """
        Register the strategy with the given execution client.

        :param client: The execution client to register.
        :raises ValueError: If client is None.
        """
        Precondition.not_none(client, 'client')

        self._exec_client = client
        self._portfolio = client.get_portfolio()
        self.account = client.get_account()
        self.log.info("Registered execution client.")

    cpdef void handle_tick(self, Tick tick):
        """"
        Updates the last held tick with the given tick, then calls on_tick()
        and passes the tick (if the strategy is running).

        :param tick: The tick received.
        """
        self._ticks[tick.symbol] = tick

        if self.is_running:
            self.on_tick(tick)

    cpdef void handle_bar(self, BarType bar_type, Bar bar):
        """"
        Updates the internal dictionary of bars with the given bar, then calls
        on_bar() and passes the arguments (if the strategy is running).

        :param bar_type: The bar type received.
        :param bar: The bar received.
        """
        if bar_type not in self._bars:
            self._bars[bar_type] = deque(maxlen=self.bar_capacity)  # type: Deque[Bar]
        self._bars[bar_type].append(bar)

        if bar_type in self._indicators:
            for updater in self._indicator_updaters[bar_type]:
                updater.update_bar(bar)

        if self.is_running:
            self.on_bar(bar_type, bar)

    cpdef void handle_event(self, Event event):
        """
        Calls on_event() and passes the event (if the strategy is running).

        :param event: The event received.
        """
        self.log.info(f"{event}")

        if self.is_running:
            self.on_event(event)

# -- DATA METHODS -------------------------------------------------------------------------------- #

    cpdef datetime time_now(self):
        """
        :return: The current time from the strategies internal clock (UTC). 
        """
        return self._clock.time_now()

    cpdef list symbols(self):
        """
        :return: All instrument symbols held by the data client -> List[Symbol].
        (available once registered with a data client).
        :raises ValueError: If the strategy has not been registered with a data client.
        """
        Precondition.not_none(self._data_client, 'data_client')

        return self._data_client.symbols

    cpdef list instruments(self):
        """
        :return: All instruments held by the data client -> List[Instrument].
        (available once registered with a data client).
        :raises ValueError: If the strategy has not been registered with a data client.
        """
        Precondition.not_none(self._data_client, 'data_client')

        return self._data_client.symbols

    cpdef Instrument get_instrument(self, Symbol symbol):
        """
        Get the instrument corresponding to the given symbol.

        :param symbol: The symbol of the instrument to get.
        :return: The instrument (if found)
        :raises ValueError: If strategy has not been registered with a data client.
        :raises KeyError: If the instrument is not found.
        """
        Precondition.not_none(self._data_client, 'data_client')

        return self._data_client.get_instrument(symbol)

    cpdef void historical_bars(self, BarType bar_type, int quantity=0):
        """
        Download the historical bars for the given parameters from the data service.
        Then pass them to all registered strategies.

        Note: Logs warning if the downloaded bars does not equal the requested quantity.

        :param bar_type: The historical bar type to download.
        :param quantity: The number of historical bars to download (>= 0)
        Note: If zero then will download bar capacity.
        :raises ValueError: If strategy has not been registered with a data client.
        :raises ValueError: If the quantity is negative (< 0).
        """
        Precondition.not_none(self._data_client, 'data_client')
        Precondition.not_negative(quantity, 'quantity')

        if quantity == 0:
            quantity = self.bar_capacity

        self._data_client.historical_bars(bar_type, quantity, self.handle_bar)

    cpdef void historical_bars_from(self, BarType bar_type, datetime from_datetime):
        """
        Download the historical bars for the given parameters from the data service.

        Note: Logs warning if the downloaded bars from datetime is greater than that given.

        :param bar_type: The historical bar type to download.
        :param from_datetime: The datetime from which the historical bars should be downloaded.
        :raises ValueError: If strategy has not been registered with a data client.
        :raises ValueError: If the from_datetime is not less than datetime.utcnow().
        """
        Precondition.not_none(self._data_client, 'data_client')
        Precondition.true(from_datetime < self._clock.time_now(),
                          'from_datetime < self.clock.time_now()')

        self._data_client.historical_bars_from(bar_type, from_datetime, self.handle_bar)

    cpdef void subscribe_bars(self, BarType bar_type):
        """
        Subscribe to bar data for the given bar type.

        :param bar_type: The bar type to subscribe to.
        :raises ValueError: If strategy has not been registered with a data client.
        """
        Precondition.not_none(self._data_client, 'data_client')

        self._data_client.subscribe_bars(bar_type, self.handle_bar)
        self.log.info(f"Subscribed to bar data for {bar_type}.")

    cpdef void unsubscribe_bars(self, BarType bar_type):
        """
        Unsubscribe from bar data for the given bar type.

        :param bar_type: The bar type to unsubscribe from.
        :raises ValueError: If strategy has not been registered with a data client.
        """
        Precondition.not_none(self._data_client, 'data_client')

        self._data_client.unsubscribe_bars(bar_type, self.handle_bar)
        self.log.info(f"Unsubscribed from bar data for {bar_type}.")

    cpdef void subscribe_ticks(self, Symbol symbol):
        """
        Subscribe to tick data for the given symbol.

        :param symbol: The tick symbol to subscribe to.
        :raises ValueError: If strategy has not been registered with a data client.
        """
        Precondition.not_none(self._data_client, 'data_client')

        self._data_client.subscribe_ticks(symbol, self.handle_tick)
        self.log.info(f"Subscribed to tick data for {symbol}.")

    cpdef void unsubscribe_ticks(self, Symbol symbol):
        """
        Unsubscribe from tick data for the given symbol.

        :param symbol: The tick symbol to unsubscribe from.
        :raises ValueError: If strategy has not been registered with a data client.
        """
        Precondition.not_none(self._data_client, 'data_client')

        self._data_client.unsubscribe_ticks(symbol, self.handle_tick)
        self.log.info(f"Unsubscribed from tick data for {symbol}.")

    cpdef list bars(self, BarType bar_type):
        """
        Get the bars for the given bar type (returns a copy of the deque).

        :param bar_type: The bar type to get.
        :return: The list of bars (List[Bar]).
        :raises KeyError: If the strategies bars dictionary does not contain the bar type.
        """
        Precondition.is_in(bar_type, self._bars, 'bar_type', 'bars')

        return list(self._bars[bar_type])

    cpdef Bar bar(self, BarType bar_type, int index):
        """
        Get the bar for the given bar type at the given index (reverse indexing, 
        pass index=0 for the last received bar).

        :param bar_type: The bar type to get.
        :param index: The index (>= 0).
        :return: The bar (if found).
        :raises ValueError: If the strategies bars dictionary does not contain the bar type.
        :raises ValueError: If the index is negative.
        :raises IndexError: If the strategies bars dictionary does not contain a bar at the given index.
        """
        Precondition.is_in(bar_type, self._bars, 'bar_type', 'bars')
        Precondition.not_negative(index, 'index')

        return self._bars[bar_type][len(self._bars[bar_type]) - 1 - index]

    cpdef Bar last_bar(self, BarType bar_type):
        """
        Get the last bar for the given bar type (if a bar has been received).

        :param bar_type: The bar type to get.
        :return: The bar (if found).
        :raises ValueError: If the strategies bars dictionary does not contain the bar type.
        :raises IndexError: If the strategies bars dictionary does not contain a bar at the given index.
        """
        Precondition.is_in(bar_type, self._bars, 'bar_type', 'bars')

        return self._bars[bar_type][len(self._bars[bar_type]) - 1]

    cpdef Tick last_tick(self, Symbol symbol):
        """
        Get the last tick held for the given symbol.

        :param symbol: The last ticks symbol.
        :return: The tick object.
        :raises KeyError: If the strategies tick dictionary does not contain a tick for the given symbol.
        """
        Precondition.is_in(symbol, self._ticks, 'symbol', 'ticks')

        return self._ticks[symbol]


# -- INDICATOR METHODS --------------------------------------------------------------------------- #

    cpdef void register_indicator(
            self,
            BarType bar_type,
            indicator: Indicator,
            update_method: Callable):
        """
        Add the given indicator to the strategy. The indicator must be from the
        inv_indicators package. Once added it will receive bars of the given
        bar type.

        :param bar_type: The indicators bar type.
        :param indicator: The indicator to set.
        :param update_method: The update method for the indicator.
        """
        Precondition.type(indicator, Indicator, 'indicator')
        Precondition.type(update_method, Callable, 'update_method')

        if bar_type not in self._indicators:
            self._indicators[bar_type] = []  # type: List[Indicator]
        self._indicators[bar_type].append(indicator)

        if bar_type not in self._indicator_updaters:
            self._indicator_updaters[bar_type] = []  # type: List[IndicatorUpdater]
        self._indicator_updaters[bar_type].append(IndicatorUpdater(indicator, update_method))

    cpdef list indicators(self, BarType bar_type):
        """
        Get the indicators list for the given bar type (returns copy).

        :param bar_type: The bar type for the indicators list.
        :return: The internally held indicators for the given bar type.
        :raises ValueError: If the strategies indicators dictionary does not contain the given bar_type.
        """
        Precondition.is_in(bar_type, self._indicators, 'bar_type', 'indicators')

        return self._indicators[bar_type].copy()

    cpdef bint indicators_initialized(self, BarType bar_type):
        """
        :return: A value indicating whether all indicators for the given bar type are initialized.
        :raises ValueError: If the strategies indicators dictionary does not contain the given bar_type.
        """
        Precondition.is_in(bar_type, self._indicators, 'bar_type', 'indicators')

        for indicator in self._indicators[bar_type]:
            if indicator.initialized is False:
                return False
        return True

    cpdef bint indicators_initialized_all(self):
        """
        :return: A value indicating whether all indicators for the strategy are initialized. 
        """
        for indicator_list in self._indicators.values():
            for indicator in indicator_list:
                if indicator.initialized is False:
                    return False
        return True


# -- MANAGEMENT METHODS -------------------------------------------------------------------------- #

    cpdef PositionId generate_position_id(self, Symbol symbol):
        """
        Generates a unique position identifier with the given symbol.

        :param symbol: The symbol.
        :return: The unique PositionId.
        """
        return self.position_id_generator.generate(symbol)

    cpdef OrderSide get_opposite_side(self, OrderSide side):
        """
        Get the opposite order side from the original side given.

        :param side: The original order side.
        :return: The opposite order side.
        """
        return OrderSide.BUY if side is OrderSide.SELL else OrderSide.SELL

    cpdef OrderSide get_flatten_side(self, MarketPosition market_position):
        """
        Get the order side needed to flatten a position from the given market position.

        :param market_position: The market position to flatten.
        :return: The order side to flatten.
        :raises KeyError: If the given market position is flat.
        """
        if market_position is MarketPosition.LONG:
            return OrderSide.SELL
        elif market_position is MarketPosition.SHORT:
            return OrderSide.BUY
        else:
            raise ValueError("Cannot flatten a FLAT position.")

    cpdef bint order_exists(self, OrderId order_id):
        """
        Return a value indicating whether an order with the given identifier exists.
        
        :param order_id: The order identifier.
        :return: True if the order exists, else False.
        :raises ValueError: If the execution client is not registered.
        """
        Precondition.not_none(self._exec_client, 'exec_client')

        return self._exec_client.order_exists(order_id)

    cpdef bint order_active(self, OrderId order_id):
        """
        Return a value indicating whether an order with the given identifier is active.
         
        :param order_id: The order identifier.
        :return: True if the order exists and is active, else False.
        :raises ValueError: If the execution client is not registered.
        :raises ValueError: If the order is not found.
        """
        Precondition.not_none(self._exec_client, 'exec_client')

        return self._exec_client.order_active(order_id)

    cpdef bint order_complete(self, OrderId order_id):
        """
        Return a value indicating whether an order with the given identifier is complete.
         
        :param order_id: The order identifier.
        :return: True if the order does not exist or is complete, else False.
        :raises ValueError: If the execution client is not registered.
        :raises ValueError: If the order is not found.
        """
        Precondition.not_none(self._exec_client, 'exec_client')

        return self._exec_client.order_complete(order_id)

    cpdef Order order(self, OrderId order_id):
        """
        Return the order with the given identifier.

        :param order_id: The order identifier.
        :return: Order.
        :raises ValueError: If the execution client is not registered.
        :raises ValueError: If the execution client does not contain an order with the given identifier.
        """
        Precondition.not_none(self._exec_client, 'exec_client')

        return self._exec_client.get_order(order_id)

    cpdef dict orders_all(self):
        """
        Return a dictionary of all orders associated with this strategy.
        
        :return: Dict[OrderId, Order]
        :raises ValueError: If the execution client is not registered.
        """
        Precondition.not_none(self._exec_client, 'exec_client')

        return self._exec_client.get_orders(self.id)

    cpdef dict orders_active(self):
        """
        Return a dictionary of all active orders associated with this strategy.
        
        :return: Dict[OrderId, Order]
        :raises ValueError: If the execution client is not registered.
        """
        Precondition.not_none(self._exec_client, 'exec_client')

        return self._exec_client.get_orders_active(self.id)

    cpdef dict orders_completed(self):
        """
        Return a dictionary of all completed orders associated with this strategy.
        
        :return: Dict[OrderId, Order]
        :raises ValueError: If the execution client is not registered.
        """
        Precondition.not_none(self._exec_client, 'exec_client')

        return self._exec_client.get_orders_completed(self.id)

    cpdef bint position_exists(self, PositionId position_id):
        """
        Return a value indicating whether a position with the given identifier exists.
        
        :param position_id: The position identifier.
        :return: True if the position exists, else False.
        :raises ValueError: If the portfolio is not registered.
        """
        Precondition.not_none(self._portfolio, 'portfolio')

        return self._portfolio.position_exists(position_id)

    cpdef Position position(self, PositionId position_id):
        """
        Return the position associated with the given id.

        :param position_id: The positions identifier.
        :return: The position with the given identifier.
        :raises ValueError: If the portfolio is not registered.
        :raises ValueError: If the portfolio does not contain a position with the given identifier.
        """
        Precondition.not_none(self._portfolio, 'portfolio')

        return self._portfolio.get_position(position_id)

    cpdef dict positions_all(self):
        """
        Return a dictionary of all positions associated with this strategy.
        
        :return: Dict[PositionId, Position]
        :raises ValueError: If the portfolio is not registered.
        """
        Precondition.not_none(self._portfolio, 'portfolio')

        return self._portfolio.get_positions(self.id)

    cpdef dict positions_active(self):
        """
        Return a dictionary of all active positions associated with this strategy.
        
        :return: Dict[PositionId, Position]
        :raises ValueError: If the portfolio is not registered.
        """
        Precondition.not_none(self._portfolio, 'portfolio')

        return self._portfolio.get_positions_active(self.id)

    cpdef dict positions_closed(self):
        """
        Return a dictionary of all closed positions associated with this strategy.
        
        :return: Dict[PositionId, Position]
        :raises ValueError: If the portfolio is not registered.
        """
        Precondition.not_none(self._portfolio, 'portfolio')

        return self._portfolio.get_positions_closed(self.id)

    cpdef bint is_flat(self):
        """
        Return a value indicating whether the strategy is completely flat (i.e no market positions
        other than FLAT across all instruments).
        
        :return: True if flat, else False.
        :raises ValueError: If the portfolio is not registered.
        """
        Precondition.not_none(self._portfolio, 'portfolio')

        return self._portfolio.is_strategy_flat(self.id)


# -- COMMAND METHODS ----------------------------------------------------------------------------- #

    cpdef void start(self):
        """
        Starts the trade strategy and calls on_start().
        """
        self.log.info(f"Starting...")
        self.is_running = True
        self.on_start()
        self.log.info(f"Running...")

    cpdef void stop(self):
        """
        Stops the trade strategy and calls on_stop().
        """
        self.log.info(f"Stopping...")
        self.on_stop()
        cdef list labels = self._clock.get_labels()
        for label in labels:
            self.log.info(f"Cancelled timer {label}.")
        self._clock.stop_all_timers()
        self.is_running = False
        self.log.info(f"Stopped.")

    cpdef void reset(self):
        """
        Reset the trade strategy by clearing all stateful internal values and
        returning it to a fresh state (strategy must not be running).
        Then calls on_reset().
        """
        if self.is_running:
            self.log.warning(f"Cannot reset a running strategy...")
            return

        self._ticks = {}  # type: Dict[Symbol, Tick]
        self._bars = {}   # type: Dict[BarType, Deque[Bar]]

        # Reset all indicators
        for indicator_list in self._indicators.values():
            [indicator.reset() for indicator in indicator_list]

        self.log.info(f"Resetting...")
        self.on_reset()
        self.log.info(f"Reset.")

    cpdef void dispose(self):
        """
        Dispose of the strategy to release system resources, on_dispose() is called.
        """
        self.log.info(f"Disposing...")
        self.on_dispose()
        self.log.info(f"Disposed.")

    cpdef void collateral_inquiry(self):
        """
        Send a collateral inquiry command to the execution service.
        """
        cdef CollateralInquiry command = CollateralInquiry(
            self._guid_factory.generate(),
            self._clock.time_now())

        self._exec_client.execute_command(command)

    cpdef void submit_order(self, Order order, PositionId position_id):
        """
        Send a submit order command with the given order and position identifier to the execution 
        service.

        :param order: The order to submit.
        :param position_id: The position identifier to associate with this order.
        """
        Precondition.not_none(self._exec_client, 'exec_client')

        self.log.info(f"Submitting {order} with {position_id}")

        cdef SubmitOrder command = SubmitOrder(
            order,
            position_id,
            self.id,
            self.name,
            self._guid_factory.generate(),
            self._clock.time_now())

        self._exec_client.execute_command(command)

    cpdef void submit_atomic_order(self, AtomicOrder order, PositionId position_id):
        """
        Send a submit atomic order command with the given order and position identifier to the 
        execution service.
        
        :param order: The atomic order to submit.
        :param position_id: The position identifier to associate with this order.
        """
        Precondition.not_none(self._exec_client, 'exec_client')

        self.log.info(f"Submitting {order} for {position_id}")

        cdef SubmitAtomicOrder command = SubmitAtomicOrder(
            order,
            position_id,
            self.id,
            self.name,
            self._guid_factory.generate(),
            self._clock.time_now())

        self._exec_client.execute_command(command)

    cpdef void modify_order(self, Order order, Price new_price):
        """
        Send a modify order command for the given order with the given new price
        to the execution service.

        :param order: The order to modify.
        :param new_price: The new price for the given order.
        """
        Precondition.not_none(self._exec_client, 'exec_client')

        self.log.info(f"Modifying {order} with new price {new_price}")

        cdef ModifyOrder command = ModifyOrder(
            order,
            new_price,
            self._guid_factory.generate(),
            self._clock.time_now())

        self._exec_client.execute_command(command)

    cpdef void cancel_order(self, Order order, str cancel_reason=None):
        """
        Send a cancel order command for the given order and cancel_reason to the
        execution service.

        :param order: The order to cancel.
        :param cancel_reason: The reason for cancellation (will be logged).
        """
        Precondition.not_none(self._exec_client, 'exec_client')

        self.log.info(f"Cancelling {order}")

        cdef CancelOrder command = CancelOrder(
            order,
            ValidString(cancel_reason),
            self._guid_factory.generate(),
            self._clock.time_now())

        self._exec_client.execute_command(command)

    cpdef void cancel_all_orders(self, str cancel_reason=None):
        """
        Send a cancel order command for all currently working orders in the
        order book with the given cancel_reason - to the execution service.

        :param cancel_reason: The reason for cancellation (will be logged).
        :raises ValueError: If the strategy has not been registered with an execution client.
        :raises ValueError: If the cancel_reason is not a valid string.
        """
        Precondition.not_none(self._exec_client, 'exec_client')

        cdef dict all_orders = self._exec_client.get_orders(self.id)
        cdef CancelOrder command

        for order_id, order in all_orders.items():
            if order.is_active:
                command = CancelOrder(
                    order,
                    ValidString(cancel_reason),
                    self._guid_factory.generate(),
                    self._clock.time_now())

                self._exec_client.execute_command(command)

    cpdef void flatten_position(self, PositionId position_id):
        """
        Flatten the position corresponding to the given identifier by generating
        the required market order, and sending it to the execution service.
        If the position is None or already FLAT will log a warning.

        :param position_id: The position identifier to flatten.
        :raises ValueError: If the position_id is not found in the position book.
        """
        Precondition.not_none(self._exec_client, 'exec_client')

        cdef Position position = self._portfolio.get_position(position_id)

        if position.market_position == MarketPosition.FLAT:
            self.log.warning(f"Cannot flatten position (the position {position_id} was already FLAT).")
            return

        cdef Order order = self.order_factory.market(
            position.symbol,
            self.get_flatten_side(position.market_position),
            position.quantity,
            Label("EXIT"),)

        self.log.info(f"Flattening {position}.")
        self.submit_order(order, position_id)

    cpdef void flatten_all_positions(self):
        """
        Flatten all positions by generating the required market orders and sending
        them to the execution service. If no positions found or a position is None
        then will log a warning.
        """
        Precondition.not_none(self._exec_client, 'exec_client')

        cdef dict positions = self._portfolio.get_positions_active(self.id)

        if len(positions) == 0:
            self.log.warning("Cannot flatten positions (no active positions to flatten).")
            return

        cdef Order order
        for position_id, position in positions.items():
            if position.market_position == MarketPosition.FLAT:
                self.log.warning(f"Cannot flatten position (the position {position_id} was already FLAT.")
                continue

            order = self.order_factory.market(
                position.symbol,
                self.get_flatten_side(position.market_position),
                position.quantity,
                Label("EXIT"))

            self.log.info(f"Flattening {position}.")
            self.submit_order(order, position_id)

    cpdef void set_time_alert(
            self,
            Label label,
            datetime alert_time):
        """
        Set a time alert for the given time. When the time is reached and the
        strategy is running, on_event() is passed the TimeEvent containing the
        alerts unique label.

        Note: The timer thread will begin immediately.

        :param label: The label for the alert (must be unique).
        :param alert_time: The time for the alert.
        :raises ValueError: If the label is not unique for this strategy.
        :raises ValueError: If the alert_time is not > than the current time (UTC).
        """
        self._clock.set_time_alert(label, alert_time, self.handle_event)
        self.log.info(f"Set time alert for {label} at {alert_time}.")

    cpdef void cancel_time_alert(self, Label label):
        """
        Cancel the time alert corresponding to the given label.

        :param label: The label for the alert to cancel.
        :raises KeyError: If the label is not found in the internal timers.
        """
        self._clock.cancel_time_alert(label)
        self.log.info(f"Cancelled time alert for {label}.")

    cpdef void set_timer(
            self,
            Label label,
            timedelta interval,
            datetime start_time,
            datetime stop_time,
            bint repeat):
        """
        Set a timer with the given interval (time delta). The timer will run from
        the start time (optionally until the stop time). When the interval is
        reached and the strategy is running, the on_event() is passed the
        TimeEvent containing the timers unique label.

        Optionally the timer can be run repeatedly whilst the strategy is running.

        Note: The timer thread will begin immediately.

        :param label: The label for the timer (must be unique).
        :param interval: The time delta interval for the timer.
        :param start_time: The start time for the timer (can be None, then starts immediately).
        :param stop_time: The stop time for the timer (can be None).
        :param repeat: The option for the timer to repeat until the strategy is stopped
        :raises ValueError: If the label is not unique.
        :raises ValueError: If the start_time is not None and not >= the current time (UTC).
        :raises ValueError: If the stop_time is not None and repeat is False.
        :raises ValueError: If the stop_time is not None and not > than the start_time.
        :raises ValueError: If the stop_time is not None and start_time plus interval is greater
        than the stop_time.
        """
        self._clock.set_timer(label, interval, start_time, stop_time, repeat, self.handle_event)
        self.log.info((f"Set timer for {label} with interval {interval}, "
                       f"starting at {start_time}, stopping at {stop_time}, repeat={repeat}."))

    cpdef void cancel_timer(self, Label label):
        """
        Cancel the timer corresponding to the given unique label.

        :param label: The label for the timer to cancel.
        :raises ValueError: If the label is not found in the internal timers.
        """
        self._clock.cancel_timer(label)
        self.log.info(f"Cancelled timer for {label}.")

# -- BACKTEST METHODS ---------------------------------------------------------------------------- #

    cpdef void change_clock(self, Clock clock):
        """
        Backtest method. Change the strategies internal clock with the given clock
        
        :param clock: The clock to change to.
        """
        self._clock = clock
        self.order_factory = OrderFactory(
            id_tag_trader=self.id_tag_trader,
            id_tag_strategy=self.id_tag_strategy,
            clock=clock)
        self.position_id_generator = PositionIdGenerator(
            id_tag_trader=self.id_tag_trader,
            id_tag_strategy=self.id_tag_strategy,
            clock=clock)

    cpdef void change_guid_factory(self, GuidFactory guid_factory):
        """
        Backtest method. Change the strategies internal GUID factory with the given GUID factory.
        
        :param guid_factory: The GUID factory to change to.
        """
        self._guid_factory = guid_factory

    cpdef void change_logger(self, Logger logger):
        """
        Backtest method. Change the strategies internal logger with the given logger.
        
        :param logger: The logger to change to.
        """
        self.log = LoggerAdapter(f"{self.name.value}", logger)

    cpdef void set_time(self, datetime time):
        """
        Backtest method. Set the strategies clock time to the given time.
        
        :param time: The time to set the clock to.
        """
        self._clock.set_time(time)

    cpdef void iterate(self, datetime time):
        """
        Backtest method. Iterate the strategies clock to the given time.
        
        :param time: The time to iterate the clock to.
        """
        self._clock.iterate_time(time)
