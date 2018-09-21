#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="strategy.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import abc
import inspect
import uuid

from collections import deque
from datetime import datetime, timedelta, timezone
from decimal import Decimal
from typing import Callable, Deque, Dict, List
from threading import Timer
from uuid import UUID

from inv_trader.core.precondition import Precondition
from inv_trader.core.logger import Logger, LoggerAdapter
from inv_trader.model.account import Account
from inv_trader.model.enums import OrderSide, MarketPosition
from inv_trader.model.events import Event, AccountEvent, OrderEvent
from inv_trader.model.events import OrderFilled, OrderPartiallyFilled
from inv_trader.model.events import TimeEvent
from inv_trader.model.objects import Symbol, Tick, BarType, Bar, Instrument
from inv_trader.model.order import Order, OrderIdGenerator, OrderFactory
from inv_trader.model.position import Position

OrderId = str
PositionId = str
Label = str
Indicator = object


class TradeStrategy:
    """
    The abstract base class for all trade strategies.
    """

    __metaclass__ = abc.ABCMeta

    def __init__(self,
                 label: str=None,
                 order_id_tag: str=None,
                 bar_capacity=1000,
                 logger: Logger=None):
        """
        Initializes a new instance of the TradeStrategy abstract class.

        :param label: The unique label for the strategy (can be None).
        :param order_id_tag: The unique order identifier tag for the strategy (can be None).
        :param bar_capacity: The capacity for the internal bar deque(s).
        :param logger: The logger (can be None, and will print).
        :raises ValueError: If the label is not a valid string.
        :raises ValueError: If the order_id_tag is not a valid string.
        :raises ValueError: If the bar_capacity is not positive (> 0).
        """
        if label is None:
            label = '0'
        if order_id_tag is None:
            order_id_tag = '0'
        Precondition.valid_string(label, 'label')
        Precondition.valid_string(order_id_tag, 'order_id_tag')
        Precondition.positive(bar_capacity, 'bar_capacity')

        self._name = self.__class__.__name__
        self._label = label
        self._id = uuid.uuid4()
        self._order_id_generator = OrderIdGenerator(order_id_tag)
        self._bar_capacity = bar_capacity
        if logger is None:
            self._log = LoggerAdapter(f"{self._name}-{self._label}")
        else:
            self._log = LoggerAdapter(f"{self._name}-{self._label}", logger)
        self._is_running = False
        self._timers = {}                # type: Dict[Label, Timer]
        self._ticks = {}                 # type: Dict[Symbol, Tick]
        self._bars = {}                  # type: Dict[BarType, Deque[Bar]]
        self._indicators = {}            # type: Dict[BarType, List[Indicator]]
        self._indicator_updaters = {}    # type: Dict[BarType, List[IndicatorUpdater]]
        self._indicator_index = {}       # type: Dict[Label, Indicator]
        self._order_book = {}            # type: Dict[OrderId, Order]
        self._order_position_index = {}  # type: Dict[OrderId, PositionId]
        self._position_book = {}         # type: Dict[PositionId, Position or None]
        self._data_client = None
        self._exec_client = None
        self._account = None  # Initialized when registered with execution client.

        self._log.info(f"Initialized.")

    def __eq__(self, other) -> bool:
        """
        Override the default equality comparison.
        """
        if isinstance(other, self.__class__):
            return str(self) == str(other)
        else:
            return False

    def __ne__(self, other):
        """
        Override the default not-equals comparison.
        """
        return not self.__eq__(other)

    def __hash__(self):
        """"
        Override the default hash implementation.
        """
        return hash((self.name, self.label))

    def __str__(self) -> str:
        """
        :return: The str() string representation of the strategy.
        """
        return f"{self._name}-{self._label}"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the strategy.
        """
        return f"<{str(self)} object at {id(self)}>"

    @abc.abstractmethod
    def on_start(self):
        """
        Called when the strategy is started.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the strategy (or just add pass).")

    @abc.abstractmethod
    def on_tick(self, tick: Tick):
        """
        Called when a tick is received by the strategy.

        :param tick: The tick received.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the strategy (or just add pass).")

    @abc.abstractmethod
    def on_bar(self, bar_type: BarType, bar: Bar):
        """
        Called when a bar is received by the strategy.

        :param bar_type: The bar type received.
        :param bar: The bar received.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the strategy (or just add pass).")

    @abc.abstractmethod
    def on_event(self, event: Event):
        """
        Called when an event is received by the strategy.

        :param event: The event received.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the strategy (or just add pass).")

    @abc.abstractmethod
    def on_stop(self):
        """
        Called when the strategy is stopped.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the strategy (or just add pass).")

    @abc.abstractmethod
    def on_reset(self):
        """
        Called when the strategy is reset.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the strategy (or just add pass).")

    @property
    def name(self) -> str:
        """
        :return: The name of the strategy.
        """
        return self._name

    @property
    def label(self) -> str:
        """
        :return: The label of the strategy.
        """
        return self._label

    @property
    def id(self) -> UUID:
        """
        :return: The unique identifier of the strategy.
        """
        return self._id

    @property
    def is_running(self) -> bool:
        """
        :return: A value indicating whether the strategy is running.
        """
        return self._is_running

    @property
    def log(self) -> LoggerAdapter:
        """
        :return: The logging adapter.
        """
        return self._log

    @property
    def all_indicators(self) -> Dict[BarType, List[Indicator]]:
        """
        :return: The indicators dictionary for the strategy.
        """
        return self._indicators

    @property
    def all_bars(self) -> Dict[BarType, Deque[Bar]]:
        """
        :return: The bars dictionary for the strategy.
        """
        return self._bars

    @property
    def ticks(self) -> Dict[Symbol, Tick]:
        """
        :return: The ticks dictionary for the strategy
        """
        return self._ticks

    @property
    def orders(self) -> Dict[OrderId, Order]:
        """
        :return: The order book for the strategy
        """
        return self._order_book

    @property
    def positions(self) -> Dict[PositionId, Position]:
        """
        :return: The position book for the strategy.
        """
        return self._position_book

    @property
    def active_orders(self) -> Dict[OrderId, Order]:
        """
        :return: The active orders for the strategy.
        """
        return ({order.id: order for order in self._order_book.values()
                 if not order.is_complete})

    @property
    def active_positions(self) -> Dict[PositionId, Position]:
        """
        :return: The active positions for the strategy.
        """
        return ({position.id: position for position in self._position_book.values()
                 if not position.is_exited})

    @property
    def completed_orders(self) -> Dict[OrderId, Order]:
        """
        :return: The completed orders for the strategy.
        """
        return ({order.id: order for order in self._order_book.values()
                 if order.is_complete})

    @property
    def completed_positions(self) -> Dict[PositionId, Position]:
        """
        :return: The completed positions for the strategy.
        """
        return ({position.id: position for position in self._position_book.values()
                 if position.is_exited})

    @property
    def is_flat(self) -> bool:
        """
        :return: A value indicating whether this strategy is completely flat
        (no positions other than FLAT).
        """
        return all(position.market_position == MarketPosition.FLAT
                   for position in self._position_book.values())

    @property
    def symbols(self) -> List[Symbol]:
        """
        :return: All instrument symbols held by the data client
        (available once registered with a data client).
        :raises ValueError: If the strategy has not been registered with a data client.
        """
        Precondition.not_none(self._data_client, 'data_client')

        return self._data_client.symbols

    @property
    def instruments(self) -> List[Instrument]:
        """
        :return: All instruments held by the data client
        (available once registered with a data client).
        :raises ValueError: If the strategy has not been registered with a data client.
        """
        Precondition.not_none(self._data_client, 'data_client')

        return self._data_client.symbols

    @property
    def account(self) -> Account:
        """
        :return: The strategies account (available once registered with an execution client).
        :raises ValueError: If the strategy has not been registered with an execution client.
        """
        Precondition.not_none(self._account, 'account')

        return self._account

    def start(self):
        """
        Starts the trade strategy and calls the on_start() method.
        """
        self._log.info(f"Starting...")
        self._is_running = True
        self.on_start()
        self._log.info(f"Running...")

    def stop(self):
        """
        Stops the trade strategy and calls the on_stop() method.
        """
        self._log.info(f"Stopping...")
        self.on_stop()
        for label, timer in self._timers.items():
            timer.cancel()
            self._log.info(f"Cancelled timer for {label}.")
        self._is_running = False
        self._log.info(f"Stopped.")

    def reset(self):
        """
        Reset the trade strategy by clearing all stateful internal values and
        returning it to a fresh state (strategy must not be running).
        """
        if self._is_running:
            self._log.warning(f"Cannot reset a running strategy...")
            return

        self._timers = {}  # type: Dict[Label, Timer]
        self._ticks = {}   # type: Dict[Symbol, Tick]
        self._bars = {}    # type: Dict[BarType, List[Bar]]

        # Reset all indicators.
        for indicator_list in self._indicators.values():
            [indicator.reset() for indicator in indicator_list]

        self._order_book = {}            # type: Dict[OrderId, Order]
        self._order_position_index = {}  # type: Dict[OrderId, PositionId]
        self._position_book = {}         # type: Dict[PositionId, Position or None]

        self.on_reset()
        self._log.info(f"Reset.")

    def get_instrument(self, symbol: Symbol) -> Instrument:
        """
        Get the instrument corresponding to the given symbol from the data client.

        :param symbol: The symbol of the instrument to get.
        :return: The instrument (if found)
        :raises ValueError: If the strategy has not been registered with a data client.
        :raises KeyError: If the instrument is not found.
        """
        Precondition.not_none(self._data_client, 'data_client')

        return self._data_client.get_instrument(symbol)

    def indicators(self, bar_type: BarType) -> List[Indicator]:
        """
        Get the indicators list for the given bar type.

        :param bar_type: The bar type for the indicators list.
        :return: The internally held indicators for the given bar type.
        :raises KeyError: If the strategies indicators dictionary does not contain the given bar_type.
        """
        if bar_type not in self._indicators:
            raise KeyError(
                f"Cannot get indicators (the indicators dictionary does not contain {bar_type}).")

        return self._indicators[bar_type]

    def indicator(self, label: str) -> Indicator:
        """
        Get the indicator for the given unique label.

        :param label: The unique label for the indicator.
        :return: The internally held indicator for the given unique label.
        :raises ValueError: If the label is not a valid string.
        :raises KeyError: If the strategies indicator dictionary does not contain the given label.
        """
        Precondition.valid_string(label, 'label')

        if label not in self._indicator_index:
            raise KeyError(
                (f"Cannot get indicator "
                 f"(the indicator dictionary does not contain the label {label})."))

        return self._indicator_index[label]

    def bars(self, bar_type: BarType) -> Deque[Bar]:
        """
        Get the bars for the given bar type.

        :param bar_type: The bar type to get.
        :return: The list of bars.
        :raises KeyError: If the strategies bars dictionary does not contain the bar type.
        """
        if bar_type not in self._bars:
            raise KeyError(
                f"Cannot get bars (the bars dictionary does not contain {bar_type}).")

        return self._bars[bar_type]

    def bar(
            self,
            bar_type: BarType,
            index: int) -> Bar:
        """
        Get the bar for the given bar type at the given index.

        :param bar_type: The bar type to get.
        :param index: The index to get (can be positive or negative but not out of range).
        :return: The bar (if found).
        :raises KeyError: If the strategies bars dictionary does not contain the bar type.
        :raises IndexError: If the strategies bars dictionary does not contain a bar at the given index.
        """
        if bar_type not in self._bars:
            raise KeyError(f"Cannot get bar (the bars dictionary does not contain {bar_type}).")

        return self._bars[bar_type][index]

    def last_tick(self, symbol: Symbol) -> Tick:
        """
        Get the last tick held for the given parameters.

        :param symbol: The last ticks symbol.
        :return: The tick object.
        :raises KeyError: If the strategies tick dictionary does not contain a tick for the given symbol.
        """
        if symbol not in self._ticks:
            raise KeyError(f"Cannot get last tick (the ticks dictionary does not contain {symbol}).")

        return self._ticks[symbol]

    def order(self, order_id: OrderId) -> Order:
        """
        Get the order from the order book with the given order_id.

        :param order_id: The order identifier.
        :return: The order (if found).
        :raises ValueError: If the order_id is not a valid string.
        :raises KeyError: If the strategies order book does not contain the order with the given id.
        """
        Precondition.valid_string(order_id, 'order_id')

        if order_id not in self._order_book:
            raise KeyError(
                f"Cannot get order (the order book does not contain the order with id {order_id}).")

        return self._order_book[order_id]

    def position(self, position_id: PositionId) -> Position:
        """
        Get the position from the positions dictionary for the given position id.

        :param position_id: The positions identifier.
        :return: The position (if found).
        :raises ValueError: If the position_id is not a valid string.
        :raises KeyError: If the strategies positions dictionary does not contain the given position_id.
        """
        Precondition.valid_string(position_id, 'position_id')

        if position_id not in self._position_book:
            raise KeyError(
                (f"Cannot get position "
                 f"(the positions dictionary does not contain the position {position_id})."))

        return self._position_book[position_id]

    def register_indicator(
            self,
            bar_type: BarType,
            indicator: Indicator,
            update_method: Callable,
            label: str):
        """
        Add the given indicator to the strategy. The indicator must be from the
        inv_indicators package. Once added it will receive bars of the given
        bar type.

        :param bar_type: The indicators bar type.
        :param indicator: The indicator to set.
        :param update_method: The update method for the indicator.
        :param label: The unique label for this indicator.
        :raises ValueError: If the label is not a valid string.
        :raises KeyError: If the given indicator label is not unique for this strategy.
        """
        Precondition.valid_string(label, 'label')

        if label in self._indicator_index:
            raise KeyError(
                "Cannot register indicator (the indicator label must be unique for this strategy).")

        if bar_type not in self._indicators:
            self._indicators[bar_type] = []  # type: List[Indicator]
        self._indicators[bar_type].append(indicator)

        if bar_type not in self._indicator_updaters:
            self._indicator_updaters[bar_type] = []  # type: List[IndicatorUpdater]
        self._indicator_updaters[bar_type].append(IndicatorUpdater(update_method))

        self._indicator_index[label] = indicator

    def set_time_alert(
            self,
            label: str,
            alert_time: datetime):
        """
        Set a time alert for the given time. When the time is reached and the
        strategy is running, on_event() is passed the TimeEvent containing the
        alerts unique label.

        Note: The timer thread will begin immediately.

        :param label: The label for the alert (must be unique).
        :param alert_time: The time for the alert.
        :raises ValueError: If the label is not a valid string.
        :raises KeyError: If the label is not unique for this strategy.
        :raises ValueError: If the alert_time is not > than the current time (UTC).
        """
        Precondition.valid_string(label, 'label')
        Precondition.true(alert_time > datetime.now(timezone.utc), 'alert_time > datetime.utcnow()')

        if label in self._timers:
            raise KeyError("Cannot set time alert (The label must be unique for this strategy).")

        timer = Timer(
            interval=(alert_time - datetime.now(timezone.utc)).total_seconds(),
            function=self._raise_time_event,
            args=[label, alert_time])

        timer.start()
        self._timers[label] = timer
        self._log.info(f"Set time alert for {label} at {alert_time}.")

    def cancel_time_alert(self, label):
        """
        Cancel the time alert corresponding to the given label.

        :param label: The label for the alert to cancel.
        :raises ValueError: If the label is not a valid string.
        :raises KeyError: If the label is not found in the internal timers.
        """
        Precondition.valid_string(label, 'label')

        if label not in self._timers:
            raise KeyError(f"Cannot cancel time alert (The label {label} was not found).")

        self._timers[label].cancel()
        del self._timers[label]
        self._log.info(f"Cancelled time alert for {label}.")

    def set_timer(
            self,
            label: str,
            interval: timedelta,
            start_time: datetime or None=None,
            stop_time: datetime or None=None,
            repeat: bool=False):
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
        :raises ValueError: If the label is not a valid string.
        :raises KeyError: If the label is not unique.
        :raises ValueError: If the start_time is not None and not >= the current time (UTC).
        :raises ValueError: If the stop_time is not None and repeat is False.
        :raises ValueError: If the stop_time is not None and not > than the start_time.
        :raises ValueError: If the stop_time is not None and start_time plus interval is greater
        than the stop_time.
        """
        Precondition.valid_string(label, 'label')

        if start_time is not None:
            Precondition.true(start_time >= datetime.now(timezone.utc), 'start_time >= datetime.utcnow()')
        else:
            start_time = datetime.now(timezone.utc)
        if stop_time is not None:
            Precondition.true(repeat, 'repeat True')
            Precondition.true(stop_time > start_time, 'stop_time > start_time')
            Precondition.true(start_time + interval <= stop_time,
                              'start_time + interval <= stop_time')

        if label in self._timers:
            raise ValueError("Cannot set timer (the label must be unique for this strategy).")

        alert_time = start_time + interval
        delay = (alert_time - datetime.now(timezone.utc)).total_seconds()
        if repeat:
            timer = Timer(
                interval=delay,
                function=self._repeating_timer,
                args=[label, alert_time, interval, stop_time])
        else:
            timer = Timer(
                interval=delay,
                function=self._raise_time_event,
                args=[label, alert_time])

        timer.start()
        self._timers[label] = timer
        self._log.info(
            (f"Set timer for {label} with interval {interval}, "
             f"starting at {start_time}, stopping at {stop_time}, repeat={repeat}."))

    def cancel_timer(self, label):
        """
        Cancel the timer corresponding to the given unique label.

        :param label: The label for the timer to cancel.
        :raises ValueError: If the label is not a valid string.
        :raises KeyError: If the label is not found in the internal timers.
        """
        Precondition.valid_string(label, 'label')

        if label not in self._timers:
            raise KeyError(f"Cannot cancel timer (The label {label} was not found).")

        self._timers[label].cancel()
        del self._timers[label]
        self._log.info(f"Cancelled timer for {label}.")

    def generate_order_id(self, symbol: Symbol) -> OrderId:
        """
        Generates a unique order identifier with the given symbol.

        :param symbol: The order symbol.
        :return: The unique order identifier.
        """
        return self._order_id_generator.generate(symbol)

    @staticmethod
    def get_opposite_side(side: OrderSide) -> OrderSide:
        """
        Get the opposite order side from the original side given.

        :param side: The original order side.
        :return: The opposite order side.
        """
        return OrderSide.BUY if side is OrderSide.SELL else OrderSide.SELL

    @staticmethod
    def get_flatten_side(market_position: MarketPosition) -> OrderSide:
        """
        Get the order side needed to flatten the position from the given market position.

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

    def collateral_inquiry(self):
        """
        Send a collateral inquiry command to the execution service.

        :raises ValueError: If the strategy has not been registered with an execution client.
        """
        Precondition.not_none(self._exec_client, 'exec_client')

        self._exec_client.collateral_inquiry()

    def submit_order(
            self,
            order: Order,
            position_id: PositionId):
        """
        Send a submit order command with the given order to the execution service.

        :param order: The order to submit.
        :param position_id: The position id to associate with this order.
        :raises ValueError: If the strategy has not been registered with an execution client.
        :raises ValueError: If the position_id is not a valid string.
        :raises KeyError: If the order_id is already contained in the order book (must be unique).
        """
        Precondition.not_none(self._exec_client, 'exec_client')
        Precondition.valid_string(position_id, 'position_id')

        if order.id in self._order_book:
            raise KeyError(f"Cannot submit order (the order id {order.id} must be unique).")

        self._order_book[order.id] = order
        self._order_position_index[order.id] = position_id

        self._log.info(f"Submitting {order}")
        self._exec_client.submit_order(order, self._id)

    def modify_order(
            self,
            order: Order,
            new_price: Decimal):
        """
        Send a modify order command for the given order with the given new price
        to the execution service.

        :param order: The order to modify.
        :param new_price: The new price for the given order.
        :raises ValueError: If the strategy has not been registered with an execution client.
        :raises ValueError: If the new_price is not positive (> 0).
        :raises KeyError: If order_id is not found in the order book.
        """
        Precondition.not_none(self._exec_client, 'exec_client')
        Precondition.positive(new_price, 'new_price')

        if order.id not in self._order_book:
            raise KeyError(f"Cannot modify order (the order id {order.id} was not found).")

        self._log.info(f"Modifying {order} with new price {new_price}")
        self._exec_client.modify_order(order, new_price)

    def cancel_order(
            self,
            order: Order,
            cancel_reason: str='NONE'):
        """
        Send a cancel order command for the given order to the execution service.

        :param order: The order to cancel.
        :param cancel_reason: The reason for cancellation (will be logged).
        :raises ValueError: If the strategy has not been registered with an execution client.
        :raises ValueError: If the cancel_reason is not a valid string.
        :raises KeyError: If the order_id is not found in the order book.
        """
        Precondition.not_none(self._exec_client, 'exec_client')
        Precondition.valid_string(cancel_reason, 'cancel_reason')

        if order.id not in self._order_book:
            raise KeyError("Cannot cancel order (the order id was not found).")

        self._log.info(f"Cancelling {order}")
        self._exec_client.cancel_order(order, cancel_reason)

    def cancel_all_orders(self, cancel_reason: str='NONE'):
        """
        Send a cancel order command for all currently working orders in the
        order book - to the execution service.

        :param cancel_reason: The reason for cancellation (will be logged).
        :raises ValueError: If the strategy has not been registered with an execution client.
        :raises ValueError: If the cancel_reason is not a valid string.
        """
        Precondition.not_none(self._exec_client, 'exec_client')
        Precondition.valid_string(cancel_reason, 'cancel_reason')

        for order in self._order_book.values():
            if not order.is_complete:
                self.cancel_order(order, cancel_reason)

    def flatten_position(self, position_id: PositionId):
        """
        Flatten the position corresponding to the given identifier by generating
        the required market order, and sending it to the execution service.

        :param position_id: The position identifier to flatten.
        :raises ValueError: If the strategy has not been registered with an execution client.
        :raises ValueError: If the position_id is not a valid string.
        :raises KeyError: If the position_id is not found in the position book.
        """
        Precondition.not_none(self._exec_client, 'exec_client')
        Precondition.valid_string(position_id, 'position_id')

        if position_id not in self._position_book.keys():
            raise KeyError(f"Cannot flatten position (the position {position_id} was not found).")

        position = self._position_book[position_id]

        if position is None:
            self._log.warning(f"Cannot flatten position (the position {position_id} was None).")
            return

        order = OrderFactory.market(
            position.symbol,
            self.generate_order_id(position.symbol),
            "FLATTEN",
            self.get_flatten_side(position.market_position),
            position.quantity)

        self.submit_order(order, position_id)

    def flatten_all_positions(self):
        """
        Flatten all positions by generating the required market orders and sending
        them to the execution service. If no positions found or a position is None
        then will log a warning.
        """
        if len(self._position_book) == 0:
            self._log.warning("Cannot flatten positions (no positions to flatten).")
            return

        for position_id, position in self._position_book.items():
            if position is None:
                self._log.warning(f"Cannot flatten position (the position {position_id} was None.")

            self.log.info(f"Flattening {position}.")
            order = OrderFactory.market(
                position.symbol,
                self.generate_order_id(position.symbol),
                "FLATTEN",
                self.get_flatten_side(position.market_position),
                position.quantity)
            self.submit_order(order, position_id)

    def _register_data_client(self, client):
        """
        Register the strategy with the data client.

        :param client: The data service client to register.
        :raises ValueError: If client is None.
        :raises TypeError: If client does not inherit from DataClient.
        """
        if client is None:
            raise ValueError("Cannot register data client (the client cannot be None).")
        if not client.__class__.__name__.endswith('DataClient'):
            raise TypeError("Cannot register data client (the client was not of type DataClient).")

        self._data_client = client

    def _register_execution_client(self, client):
        """
        Register the strategy with the execution client.

        :param client: The execution client to register.
        :raises ValueError: If client is None.
        :raises TypeError: If client does not inherit from ExecutionClient.
        """
        if client is None:
            raise ValueError("Cannot register execution client (the client cannot be None).")
        if client.__class__.__mro__[-2].__name__ != 'ExecutionClient':
            raise TypeError(
                ("Cannot register execution client "
                 "(the given client does not inherit from the ExecutionClient base class)."))

        self._exec_client = client
        self._account = client.account

    def _update_ticks(self, tick: Tick):
        """"
        Updates the last held tick with the given tick then calls the on_tick
        method for the inheriting class.

        :param tick: The tick received.
        """
        # Update the internal ticks.
        self._ticks[tick.symbol] = tick

        if self._is_running:
            self.on_tick(tick)

    def _update_bars(
            self,
            bar_type: BarType,
            bar: Bar):
        """"
        Updates the internal dictionary of bars with the given bar, then calls the
        on_bar method for the inheriting class.

        :param bar_type: The bar type received.
        :param bar: The bar received.
        """
        # Update the bars.
        if bar_type not in self._bars:
            self._bars[bar_type] = deque(maxlen=self._bar_capacity)  # type: Deque[Bar]
        self._bars[bar_type].append(bar)

        # Update the indicators.
        if bar_type in self._indicators:
            self._update_indicators(bar_type, bar)

        if self._is_running:
            self.on_bar(bar_type, bar)

    def _update_indicators(
            self,
            bar_type: BarType,
            bar: Bar):
        """
        Updates the internal indicators of the given bar type with the given bar.

        :param bar_type: The bar type to update.
        :param bar: The bar for update.
        """
        if bar_type not in self._indicators:
            # No indicators to update with this bar.
            return

        # For each updater matching the given bar type -> update with the bar.
        [updater.update(bar) for updater in self._indicator_updaters[bar_type]]

    def _update_events(self, event: Event):
        """
        Updates the strategy with the given event.

        :param event: The event received.
        """
        self._log.info(str(event))

        # Order events.
        if isinstance(event, OrderEvent):
            order_id = event.order_id
            if order_id in self._order_book:
                order = self._order_book[order_id]
                order.apply(event)
            else:
                self._log.warning("The events order id was not found in the order book.")

            # Position events.
            if isinstance(event, OrderFilled) or isinstance(event, OrderPartiallyFilled):
                if event.order_id in self._order_position_index:
                    position_id = self._order_position_index[event.order_id]

                    if position_id not in self._position_book:
                        opened_position = Position(
                            event.symbol,
                            position_id,
                            event.execution_time)
                        opened_position.apply(event)
                        self._position_book[position_id] = opened_position
                        self._log.info(f"Opened {opened_position}")
                    else:
                        position = self._position_book[position_id]
                        position.apply(event)

                        if position.is_exited:
                            self._log.info(f"Closed {position}")
                        else:
                            self._log.info(f"Modified {self._position_book[position_id]}")
                else:
                    self._log.warning(
                        "The events order id was not found in the order position index.")

        # Account Events.
        elif isinstance(event, AccountEvent):
            self._account.apply(event)

        if self._is_running:
            self.on_event(event)

    def _raise_time_event(
            self,
            label: str,
            alert_time: datetime):
        """
        Create a new TimeEvent and pass it into the _update_events() method.
        """
        self._log.debug(f"Raising time event for {label}.")
        self._update_events(TimeEvent(label, uuid.uuid4(), alert_time))
        del self._timers[label]

    def _repeating_timer(
            self,
            label: str,
            alert_time: datetime,
            interval: timedelta,
            stop_time: datetime or None):
        """
        Create a new TimeEvent and pass it into the _update_events() method.
        Then start a timer for the next time event.
        """
        self._update_events(TimeEvent(label, uuid.uuid4(), alert_time))

        if stop_time is not None and alert_time + interval >= stop_time:
            self._log.info(f"Stop time reached for timer {label}.")
            self._timers[label].cancel()
            del self._timers[label]
            return

        next_alert_time = alert_time + interval
        delay = (next_alert_time - datetime.now(timezone.utc)).total_seconds()
        timer = Timer(
            interval=delay,
            function=self._repeating_timer,
            args=[label, next_alert_time, interval, stop_time])
        timer.start()
        self._timers[label] = timer
        self._log.debug(f"Continuing timer for {label}...")


POINT = 'point'
PRICE = 'price'
MID = 'mid'
OPEN = 'open'
HIGH = 'high'
LOW = 'low'
CLOSE = 'close'
VOLUME = 'volume'
TIMESTAMP = 'timestamp'


class IndicatorUpdater:
    """
    Provides an adapter for updating an indicator with a bar. When instantiated
    with a live indicator update method, the updater will inspect the method and
    construct the required parameter list for updates.
    """

    def __init__(self, update_method: Callable):
        """
        Initializes a new instance of the IndicatorUpdater class.

        :param update_method: The indicators update method.
        """
        self._update_method = update_method
        self._update_params = []

        param_map = {
            POINT: CLOSE,
            PRICE: CLOSE,
            MID: CLOSE,
            OPEN: OPEN,
            HIGH: HIGH,
            LOW: LOW,
            CLOSE: CLOSE,
            TIMESTAMP: TIMESTAMP
        }

        for param in inspect.signature(update_method).parameters:
            self._update_params.append(param_map[param])

    def update(self, bar: Bar):
        """
        Passes the needed values from the given bar to the indicator update
        method as a list of arguments.

        :param bar: The update bar.
        """
        args = [bar.__getattribute__(param) for param in self._update_params]
        self._update_method(*args)
