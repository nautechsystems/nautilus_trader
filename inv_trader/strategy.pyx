#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="strategy.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

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
from inv_trader.model.events cimport Event, AccountEvent, OrderEvent
from inv_trader.model.events cimport OrderRejected, OrderCancelReject, OrderFilled, OrderPartiallyFilled
from inv_trader.model.identifiers cimport GUID, Label, OrderId, PositionId
from inv_trader.model.objects cimport Symbol, Tick, BarType, Bar, Instrument
from inv_trader.model.order cimport Order, OrderIdGenerator, OrderFactory
from inv_trader.model.position cimport Position
from inv_trader.tools cimport IndicatorUpdater
Indicator = object


cdef class TradeStrategy:
    """
    The abstract base class for all trade strategies.
    """

    def __init__(self,
                 str label='0',
                 str order_id_tag='0',
                 int bar_capacity=1000,
                 Clock clock=LiveClock(),
                 Logger logger=None):
        """
        Initializes a new instance of the TradeStrategy abstract class.

        :param label: The optional unique label for the strategy.
        :param order_id_tag: The optional unique order identifier tag for the strategy.
        :param bar_capacity: The capacity for the internal bar deque(s).
        :param clock: The internal clock for the strategy.
        :param logger: The logger (can be None, and will print).
        :raises ValueError: If the label is not a valid string.
        :raises ValueError: If the order_id_tag is not a valid string.
        :raises ValueError: If the bar_capacity is not positive (> 0).
        """
        Precondition.valid_string(label, 'label')
        Precondition.valid_string(order_id_tag, 'order_id_tag')
        Precondition.positive(bar_capacity, 'bar_capacity')

        self._clock = clock
        if logger is None:
            self.log = LoggerAdapter(f"{self.name}-{self.label}")
        else:
            self.log = LoggerAdapter(f"{self.name}-{self.label}", logger)
        self.name = self.__class__.__name__
        self.label = label
        self.order_id_tag = order_id_tag
        self.id = GUID(uuid.uuid4())
        self._order_id_generator = OrderIdGenerator(order_id_tag=order_id_tag, clock=self._clock)
        self.order_factory = OrderFactory()
        self.bar_capacity = bar_capacity
        self.is_running = False
        self._ticks = {}                 # type: Dict[Symbol, Tick]
        self._bars = {}                  # type: Dict[BarType, Deque[Bar]]
        self._indicators = {}            # type: Dict[BarType, List[Indicator]]
        self._indicator_updaters = {}    # type: Dict[BarType, List[IndicatorUpdater]]
        self._order_book = {}            # type: Dict[OrderId, Order]
        self._order_position_index = {}  # type: Dict[OrderId, PositionId]
        self._position_book = {}         # type: Dict[PositionId, Position or None]
        self._data_client = None
        self._exec_client = None
        self.account = None  # Initialized when registered with execution client.

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
        return hash((self.name, self.label))

    def __str__(self) -> str:
        """
        :return: The str() string representation of the strategy.
        """
        return f"{self.name}-{self.label}"

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
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the strategy (or just add pass).")

    cpdef void on_tick(self, Tick tick):
        """
        Called when a tick is received by the strategy.

        :param tick: The tick received.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the strategy (or just add pass).")

    cpdef void on_bar(self, BarType bar_type, Bar bar):
        """
        Called when a bar is received by the strategy.

        :param bar_type: The bar type received.
        :param bar: The bar received.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the strategy (or just add pass).")

    cpdef void on_event(self, Event event):
        """
        Called when an event is received by the strategy.

        :param event: The event received.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the strategy (or just add pass).")

    cpdef void on_stop(self):
        """
        Called when the strategy is stopped.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the strategy (or just add pass).")

    cpdef void on_reset(self):
        """
        Called when the strategy is reset.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the strategy (or just add pass).")


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

        # List already copied in date client
        return self._data_client.symbols

    cpdef list instruments(self):
        """
        :return: All instruments held by the data client -> List[Instrument].
        (available once registered with a data client).
        :raises ValueError: If the strategy has not been registered with a data client.
        """
        Precondition.not_none(self._data_client, 'data_client')

        # List already copied in date client
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

    cpdef void historical_bars(self, BarType bar_type, int quantity):
        """
        Download the historical bars for the given parameters from the data service.
        Then pass them to all registered strategies.

        Note: Logs warning if the downloaded bars does not equal the requested quantity.

        :param bar_type: The historical bar type to download.
        :param quantity: The number of historical bars to download
        (if None then will download to bar_capacity).
        :raises ValueError: If strategy has not been registered with a data client.
        :raises ValueError: If the amount is not positive (> 0).
        """
        Precondition.not_none(self._data_client, 'data_client')
        if quantity <= 0:
            quantity = self.bar_capacity
        Precondition.positive(quantity, 'quantity')

        self._data_client.historical_bars(bar_type, quantity, self._update_bars)

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

        self._data_client.historical_bars_from(bar_type, from_datetime, self._update_bars)

    cpdef void subscribe_bars(self, BarType bar_type):
        """
        Subscribe to bar data for the given bar type.

        :param bar_type: The bar type to subscribe to.
        :raises ValueError: If strategy has not been registered with a data client.
        """
        Precondition.not_none(self._data_client, 'data_client')

        self._data_client.subscribe_bars(bar_type, self._update_bars)
        self.log.info(f"Subscribed to bar data for {bar_type}.")

    cpdef void unsubscribe_bars(self, BarType bar_type):
        """
        Unsubscribe from bar data for the given bar type.

        :param bar_type: The bar type to unsubscribe from.
        :raises ValueError: If strategy has not been registered with a data client.
        """
        Precondition.not_none(self._data_client, 'data_client')

        self._data_client.unsubscribe_bars(bar_type, self._update_bars)
        self.log.info(f"Unsubscribed from bar data for {bar_type}.")

    cpdef void subscribe_ticks(self, Symbol symbol):
        """
        Subscribe to tick data for the given symbol.

        :param symbol: The tick symbol to subscribe to.
        :raises ValueError: If strategy has not been registered with a data client.
        """
        Precondition.not_none(self._data_client, 'data_client')

        self._data_client.subscribe_ticks(symbol, self._update_ticks)
        self.log.info(f"Subscribed to tick data for {symbol}.")

    cpdef void unsubscribe_ticks(self, Symbol symbol):
        """
        Unsubscribe from tick data for the given symbol.

        :param symbol: The tick symbol to unsubscribe from.
        :raises ValueError: If strategy has not been registered with a data client.
        """
        Precondition.not_none(self._data_client, 'data_client')

        self._data_client.unsubscribe_ticks(symbol, self._update_ticks)
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

    cpdef register_indicator(
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

        # Closures not yet supported so can't do one liner
        cpdef bint initialized = True
        for indicator in self._indicators[bar_type]:
            if indicator.initialized is False:
                initialized = False
        return initialized

    cpdef bint all_indicators_initialized(self):
        """
        :return: A value indicating whether all indicators for the strategy are initialized. 
        """
        # Closures not yet supported so can't do one liner
        cpdef bint initialized = True
        for indicator_list in self._indicators.values():
            for indicator in indicator_list:
                if indicator.initialized is False:
                    initialized = False
        return initialized


# -- ORDER MANAGEMENT METHODS -------------------------------------------------------------------- #

    cpdef OrderId generate_order_id(self, Symbol symbol):
        """
        Generates a unique order identifier with the given symbol.

        :param symbol: The order symbol.
        :return: The unique order identifier.
        """
        return self._order_id_generator.generate(symbol)

    cpdef OrderSide get_opposite_side(self, OrderSide side):
        """
        Get the opposite order side from the original side given.

        :param side: The original order side.
        :return: The opposite order side.
        """
        return OrderSide.BUY if side is OrderSide.SELL else OrderSide.SELL

    cpdef get_flatten_side(self, MarketPosition market_position):
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

    cpdef Order order(self, OrderId order_id):
        """
        Get the order from the order book with the given order_id.

        :param order_id: The order identifier.
        :return: The order (if found).
        :raises ValueError: If the order_id is not a valid string.
        :raises KeyError: If the strategies order book does not contain the order with the given id.
        """
        Precondition.is_in(order_id, self._order_book, 'order.id', 'order_book')

        return self._order_book[order_id]

    cpdef Position position(self, PositionId position_id):
        """
        Get the position from the positions dictionary for the given position id.

        :param position_id: The positions identifier.
        :return: The position (if found).
        :raises ValueError: If the position_id is not a valid string.
        :raises ValueError: If the strategies positions dictionary does not contain the given position_id.
        """
        Precondition.is_in(position_id, self._position_book, 'order.id', 'position_book')

        return self._position_book[position_id]

    cpdef dict active_orders(self):
        """
        :return: All active orders for the strategy.
        """
        return ({order.id : order for order in self._order_book.values()
                 if not order.is_complete})

    cpdef dict active_positions(self):
        """
        :return: All active positions for the strategy.
        """
        return ({position.id: position for position in self._position_book.values()
                 if not position.is_exited})

    cpdef dict completed_orders(self):
        """
        :return: All completed orders for the strategy.
        """
        return ({order.id: order for order in self._order_book.values()
                 if order.is_complete})

    cpdef dict completed_positions(self):
        """
        :return: All completed positions for the strategy.
        """
        return ({position.id: position for position in self._position_book.values()
                 if position.is_exited})

    cpdef bint is_flat(self):
        """
        :return: A value indicating whether this strategy is completely flat
        (no positions other than FLAT) across all instruments.
        """
        cdef bint is_flat = True
        for position in self._position_book.values():
            if position.market_position != MarketPosition.FLAT:
                is_flat = False
        return is_flat


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

        self._ticks = {}                 # type: Dict[Symbol, Tick]
        self._bars = {}                  # type: Dict[BarType, Deque[Bar]]

        # Reset all indicators.
        for indicator_list in self._indicators.values():
            [indicator.reset() for indicator in indicator_list]

        self._order_book = {}            # type: Dict[OrderId, Order]
        self._order_position_index = {}  # type: Dict[OrderId, PositionId]
        self._position_book = {}         # type: Dict[PositionId, Position or None]

        self.on_reset()
        self.log.info(f"Reset.")

    cpdef collateral_inquiry(self):
        """
        Send a collateral inquiry command to the execution service.

        :raises ValueError: If the strategy has not been registered with an execution client.
        """
        Precondition.not_none(self._exec_client, 'exec_client')

        self._exec_client.collateral_inquiry()

    cpdef submit_order(self, Order order, PositionId position_id):
        """
        Send a submit order command with the given order to the execution service.

        :param order: The order to submit.
        :param position_id: The position id to associate with this order.
        :raises ValueError: If the strategy has not been registered with an execution client.
        :raises ValueError: If the order_id is already contained in the order book (must be unique).
        """
        Precondition.not_none(self._exec_client, 'exec_client')
        Precondition.not_in(order.id, self._order_book, 'order.id', 'order_book')

        self._order_book[order.id] = order
        self._order_position_index[order.id] = position_id

        self.log.info(f"Submitting {order}")
        self._exec_client.submit_order(order, self.id)

    cpdef modify_order(self, Order order, new_price):
        """
        Send a modify order command for the given order with the given new price
        to the execution service.

        :param order: The order to modify.
        :param new_price: The new price for the given order.
        :raises ValueError: If the strategy has not been registered with an execution client.
        :raises ValueError: If the new_price is not positive (> 0).
        :raises ValueError: If order_id is not found in the order book.
        """
        Precondition.not_none(self._exec_client, 'exec_client')
        Precondition.positive(new_price, 'new_price')
        Precondition.is_in(order.id, self._order_book, 'order.id', 'order_book')

        self.log.info(f"Modifying {order} with new price {new_price}")
        self._exec_client.modify_order(order, new_price)

    cpdef cancel_order(self, Order order, str cancel_reason):
        """
        Send a cancel order command for the given order and cancel_reason to the
        execution service.

        :param order: The order to cancel.
        :param cancel_reason: The reason for cancellation (will be logged).
        :raises ValueError: If the strategy has not been registered with an execution client.
        :raises ValueError: If the cancel_reason is not a valid string.
        :raises ValueError: If the order_id is not found in the order book.
        """
        Precondition.not_none(self._exec_client, 'exec_client')
        Precondition.valid_string(cancel_reason, 'cancel_reason')
        Precondition.is_in(order.id, self._order_book, 'order.id', 'order_book')

        self.log.info(f"Cancelling {order}")
        self._exec_client.cancel_order(order, cancel_reason)

    cpdef cancel_all_orders(self, str cancel_reason):
        """
        Send a cancel order command for all currently working orders in the
        order book with the given cancel_reason - to the execution service.

        :param cancel_reason: The reason for cancellation (will be logged).
        :raises ValueError: If the strategy has not been registered with an execution client.
        :raises ValueError: If the cancel_reason is not a valid string.
        """
        Precondition.not_none(self._exec_client, 'exec_client')
        Precondition.valid_string(cancel_reason, 'cancel_reason')

        for order in self._order_book.values():
            if not order.is_complete:
                self.cancel_order(order, cancel_reason)

    cpdef flatten_position(self, PositionId position_id):
        """
        Flatten the position corresponding to the given identifier by generating
        the required market order, and sending it to the execution service.
        If the position is None or already FLAT will log a warning.

        :param position_id: The position identifier to flatten.
        :raises ValueError: If the strategy has not been registered with an execution client.
        :raises ValueError: If the position_id is not a valid string.
        :raises ValueError: If the position_id is not found in the position book.
        """
        Precondition.not_none(self._exec_client, 'exec_client')
        Precondition.is_in(position_id, self._position_book, 'position_id', 'position_book')

        cdef Position position = self._position_book[position_id]

        if position is None:
            self.log.warning(f"Cannot flatten position (the position {position_id} was None).")
            return

        if position.market_position == MarketPosition.FLAT:
            self.log.warning(
                f"Cannot flatten position (the position {position_id} was already FLAT).")
            return

        cdef Order order = self.order_factory.market(
            position.symbol,
            self.generate_order_id(position.symbol),
            Label("FLATTEN"),
            self.get_flatten_side(position.market_position),
            position.quantity)

        self.submit_order(order, position_id)

    cpdef flatten_all_positions(self):
        """
        Flatten all positions by generating the required market orders and sending
        them to the execution service. If no positions found or a position is None
        then will log a warning.
        """
        if len(self.active_positions()) == 0:
            self.log.warning("Cannot flatten positions (no active positions to flatten).")
            return

        for position_id, position in self._position_book.items():
            if position is None:
                self.log.warning(f"Cannot flatten position (the position {position_id} was None.")
                continue
            if position.market_position == MarketPosition.FLAT:
                continue

            self.log.info(f"Flattening {position}.")
            order = self.order_factory.market(
                position.symbol,
                self.generate_order_id(position.symbol),
                Label("FLATTEN"),
                self.get_flatten_side(position.market_position),
                position.quantity)
            self.submit_order(order, position_id)

    cpdef set_time_alert(
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
        self._clock.set_time_alert(label, alert_time, self._update_events)
        self.log.info(f"Set time alert for {label} at {alert_time}.")

    cpdef cancel_time_alert(self, Label label):
        """
        Cancel the time alert corresponding to the given label.

        :param label: The label for the alert to cancel.
        :raises KeyError: If the label is not found in the internal timers.
        """
        self._clock.cancel_time_alert(label)
        self.log.info(f"Cancelled time alert for {label}.")

    cpdef set_timer(
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
        self._clock.set_timer(label, interval, start_time, stop_time, repeat, self._update_events)
        self.log.info(
            (f"Set timer for {label} with interval {interval}, "
             f"starting at {start_time}, stopping at {stop_time}, repeat={repeat}."))

    cpdef cancel_timer(self, Label label):
        """
        Cancel the timer corresponding to the given unique label.

        :param label: The label for the timer to cancel.
        :raises ValueError: If the label is not found in the internal timers.
        """
        self._clock.cancel_timer(label)
        self.log.info(f"Cancelled timer for {label}.")


# -- INTERNAL METHODS ---------------------------------------------------------------------------- #

    cpdef _register_data_client(self, DataClient client):
        """
        Register the strategy with the given data client.

        :param client: The data service client to register.
        :raises ValueError: If client is None.
        :raises TypeError: If client does not inherit from DataClient.
        """
        Precondition.not_none(client, 'client')

        self._data_client = client

    cpdef _register_execution_client(self, ExecutionClient client):
        """
        Register the strategy with the given execution client.

        :param client: The execution client to register.
        :raises ValueError: If client is None.
        :raises TypeError: If client does not inherit from ExecutionClient.
        """
        Precondition.not_none(client, 'client')

        self._exec_client = client
        self.account = client.account

    cpdef void _update_ticks(self, Tick tick):
        """"
        Updates the last held tick with the given tick, then calls on_tick()
        for the inheriting class.

        :param tick: The tick received.
        """
        self._ticks[tick.symbol] = tick

        if self.is_running:
            self.on_tick(tick)

    cpdef void _update_bars(self, BarType bar_type, Bar bar):
        """"
        Updates the internal dictionary of bars with the given bar, then calls the
        on_bar method for the inheriting class.

        :param bar_type: The bar type received.
        :param bar: The bar received.
        """
        if bar_type not in self._bars:
            self._bars[bar_type] = deque(maxlen=self.bar_capacity)  # type: Deque[Bar]
        self._bars[bar_type].append(bar)

        if bar_type in self._indicators:
            self._update_indicators(bar_type, bar)

        if self.is_running:
            self.on_bar(bar_type, bar)

    cpdef void _update_indicators(self, BarType bar_type, Bar bar):
        """
        Updates the internal indicators of the given bar type with the given bar.

        :param bar_type: The bar type to update.
        :param bar: The bar for update.
        """
        if bar_type not in self._indicators:
            # No indicators to update with this bar.
            return

        for updater in self._indicator_updaters[bar_type]:
            updater.update_bar(bar)

    cpdef void _update_events(self, Event event):
        """
        Updates the strategy with the given event, then calls on_event() if the
        strategy is running.

        :param event: The event received.
        """
        # Order events
        if isinstance(event, OrderEvent):
            Precondition.is_in(event.order_id, self._order_book, 'order_id', 'order_book')

            order = self._order_book[event.order_id]
            order.apply(event)

            if isinstance(event, OrderRejected):
                self.log.warning(f"{event} {event.rejected_reason}")

            elif isinstance(event, OrderCancelReject):
                self.log.warning(f"{event} {event.cancel_reject_reason} {event.cancel_reject_response}")

            # Position events
            elif isinstance(event, OrderFilled) or isinstance(event, OrderPartiallyFilled):
                Precondition.is_in(event.order_id, self._order_position_index, 'order_id', 'order_position_index')

                self.log.info(str(event))
                position_id = self._order_position_index[event.order_id]

                if position_id not in self._position_book:
                    opened_position = Position(
                        event.symbol,
                        position_id,
                        event.execution_time)
                    opened_position.apply(event)
                    self._position_book[position_id] = opened_position
                    self.log.info(f"Opened {opened_position}")
                else:
                    position = self._position_book[position_id]
                    position.apply(event)

                    if position.is_exited:
                            self.log.info(f"Closed {position}")
                    else:
                        self.log.info(f"Modified {self._position_book[position_id]}")
            else:
                self.log.info(str(event))

        # Account events
        elif isinstance(event, AccountEvent):
            self.account.apply(event)
            self.log.info(str(event))

        if self.is_running:
            self.on_event(event)

    cpdef void _change_clock(self, Clock clock):
        """
        Change the strategies internal clock with the given clock
        
        :param clock: The clock to change to.
        """
        self._clock = clock
        self._order_id_generator = OrderIdGenerator(self.order_id_tag, clock=self._clock)

    cpdef void _change_logger(self, Logger logger):
        """
        Change the strategies internal logger with the given logger.
        
        :param logger: The logger to change to.
        """
        self.log = LoggerAdapter(f"{self.name}-{self.label}", logger)

    cpdef void _set_time(self, datetime time):
        """
        Set the strategies clock time to the given time.
        
        :param time: The time to set the clock to.
        """
        self._clock.set_time(time)

    cpdef void _iterate(self, datetime time):
        """
        Iterate the strategies clock to the given time.
        
        :param time: The time to iterate the clock to.
        """
        self._clock.iterate_time(time)
