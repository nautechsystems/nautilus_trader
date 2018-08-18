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

from datetime import datetime, timedelta
from decimal import Decimal
from typing import List, Dict, KeysView, Callable
from uuid import UUID

from inv_trader.core.checks import typechecking
from inv_trader.model.enums import OrderSide, MarketPosition
from inv_trader.model.objects import Symbol, Tick, BarType, Bar
from inv_trader.model.order import Order
from inv_trader.model.position import Position
from inv_trader.model.account import Account
from inv_trader.model.events import Event, AccountEvent, OrderEvent
from inv_trader.model.events import OrderFilled, OrderPartiallyFilled
from inv_trader.factories import OrderIdGenerator

# Constants
OrderId = str
Label = str
Indicator = object

POINT = 'point'
PRICE = 'price'
MID = 'mid'
OPEN = 'open'
HIGH = 'high'
LOW = 'low'
CLOSE = 'close'
VOLUME = 'volume'
TIMESTAMP = 'timestamp'


class TradeStrategy:
    """
    The abstract base class for all trade strategies.
    """

    __metaclass__ = abc.ABCMeta

    @typechecking
    def __init__(self,
                 label: str=None,
                 order_id_tag: str=''):
        """
        Initializes a new instance of the TradeStrategy abstract class.

        :param: label: The unique label for the strategy (can be None).
        :param: order_id_tag: The unique order identifier tag for the strategy (can be empty).
        """
        if label is None:
            label = ''

        self._name = self.__class__.__name__
        self._id = uuid.uuid4()
        self._label = label
        self._order_id_generator = OrderIdGenerator(order_id_tag)
        self._is_running = False
        self._ticks = {}               # type: Dict[Symbol, Tick]
        self._bars = {}                # type: Dict[BarType, List[Bar]]
        self._indicators = {}          # type: Dict[BarType, List[Indicator]]
        self._indicator_updaters = {}  # type: Dict[BarType, List[IndicatorUpdater]]
        self._indicator_index = {}     # type: Dict[Label, Indicator]
        self._order_book = {}          # type: Dict[OrderId, Order]
        self._positions = {}           # type: Dict[Symbol, Position]
        self._account = Account()
        self._exec_client = None

        self._log(f"Initialized.")

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
    def id(self) -> UUID:
        """
        :return: The unique identifier of the strategy.
        """
        return self._id

    @property
    def label(self) -> str:
        """
        :return: The label of the strategy.
        """
        return self._label

    @property
    def is_running(self) -> bool:
        """
        :return: A value indicating whether the strategy is running.
        """
        return self._is_running

    @property
    def indicator_labels(self) -> KeysView[Label]:
        """
        :return: The indicator label list for the strategy (should be distinct).
        """
        return self._indicator_index.keys()

    @property
    def all_indicators(self) -> Dict[BarType, List[Indicator]]:
        """
        :return: The indicators dictionary for the strategy.
        """
        return self._indicators

    @property
    def all_bars(self) -> Dict[BarType, List[Bar]]:
        """
        :return: The bars dictionary for the strategy.
        """
        return self._bars

    @property
    def ticks(self) -> Dict[Symbol, Tick]:
        """
        :return: The internally held ticks dictionary for the strategy
        """
        return self._ticks

    @property
    def orders(self) -> Dict[OrderId, Order]:
        """
        :return: The entire order book for the strategy
        """
        return self._order_book

    @property
    def positions(self) -> Dict[Symbol, Position]:
        """
        :return: The entire dictionary of positions.
        """
        return self._positions

    @property
    def account(self) -> Account:
        """
        :return: The strategies account.
        """
        return self._account

    @typechecking
    def indicators(self, bar_type: BarType) -> List[Indicator]:
        """
        Get the indicators list for the given bar type.

        :param: The bar type for the indicators list.
        :return: The internally held indicators for the given bar type.
        :raises: KeyError: If the strategy does not contain indicators for the bar type.
        """
        # Preconditions
        if bar_type not in self._indicators:
            raise KeyError(f"The indicators dictionary does not contain {bar_type}.")

        return self._indicators[bar_type]

    @typechecking
    def indicator(self, label: str) -> Indicator:
        """
        Get the indicator for the given unique label.

        :param: label: The unique label for the indicator.
        :return: The internally held indicator for the given unique label.
        :raises: KeyError: If the strategy does not contain the indicator label.
        """
        # Preconditions
        if label not in self._indicator_index:
            raise KeyError(f"The indicator dictionary does not contain the label {label}.")

        return self._indicator_index[label]

    # @typechecking: cannot type check generics?
    def bars(self, bar_type: BarType) -> List[Bar]:
        """
        Get the bars for the given bar type.

        :param bar_type: The bar type to get.
        :return: The list of bars.
        :raises: KeyError: If the strategy does not contain the bar type..
        """
        # Preconditions
        if bar_type not in self._bars:
            raise KeyError(f"The bars dictionary does not contain {bar_type}.")

        return self._bars[bar_type]

    @typechecking
    def bar(
            self,
            bar_type: BarType,
            index: int) -> Bar:
        """
        Get the bar for the given bar type at the given index (reverse indexed, 0 is last bar).

        :param bar_type: The bar type to get.
        :param index: The index to get.
        :return: The found bar.
        :raises: KeyError: If the strategy does not contain the bar type.
        :raises: ValueError: If the index is negative.
        """
        # Preconditions
        if bar_type not in self._bars:
            raise KeyError(f"The bars dictionary does not contain {bar_type}.")
        if index < 0:
            raise ValueError("The index cannot be negative.")

        return self._bars[bar_type][index]

    @typechecking
    def last_tick(self, symbol: Symbol) -> Tick:
        """
        Get the last tick held for the given parameters.

        :param symbol: The last tick symbol.
        :return: The tick object.
        :raises: KeyError: If the strategy does not contain a tick for the symbol and venue string.
        """
        # Preconditions
        if symbol not in self._ticks:
            raise KeyError(f"The ticks dictionary does not contain {symbol}.")

        return self._ticks[symbol]

    @typechecking
    def order(self, order_id: OrderId) -> Order:
        """
        Get the order from the order book with the given order_id.

        :param order_id: The order identifier.
        :return: The order (if found).
        :raises: KeyError: If the strategy does not contain the order with the requested id.
        """
        # Preconditions
        if order_id not in self._order_book:
            raise KeyError(f"The order book does not contain the order with id {order_id}.")

        return self._order_book[order_id]

    @typechecking
    def position(self, symbol: Symbol) -> Position:
        """
        Get the position from the positions dictionary for the given symbol.

        :param symbol: The positions symbol.
        :return: The position (if found).
        :raises: KeyError: If the strategy does not contain a position for the given symbol.
        """
        # Preconditions
        if symbol not in self._positions:
            raise KeyError(
                f"The positions dictionary does not contain a position for symbol {symbol}.")

        return self._positions[symbol]

    @typechecking  # @ typechecking: indicator checked in preconditions.
    def add_indicator(
            self,
            bar_type: BarType,
            indicator: Indicator,
            update_method: Callable,
            label: Label):
        """
        Add the given indicator to the strategy. It will receive bars of the
        given bar type. The indicator must be from the inv_indicators package.

        :param bar_type: The indicators bar type.
        :param indicator: The indicator to set.
        :param update_method: The update method for the indicator.
        :param label: The unique label for this indicator.
        """
        # Preconditions
        if indicator is None:
            raise ValueError("The indicator cannot be None.")
        if indicator.__class__.__mro__[-2].__name__ != 'Indicator':
            raise TypeError("The indicator must inherit from the Indicator base class.")
        if label in self._indicator_index.keys():
            raise KeyError("The indicator label must be unique for this strategy.")

        if bar_type not in self._indicators:
            self._indicators[bar_type] = []  # type: List[Indicator]
        self._indicators[bar_type].append(indicator)

        if bar_type not in self._indicator_updaters:
            self._indicator_updaters[bar_type] = []  # type: List[IndicatorUpdater]
        self._indicator_updaters[bar_type].append(IndicatorUpdater(update_method))

        self._indicator_index[label] = indicator

    @typechecking
    def set_timer(
            self,
            label: Label,
            step: timedelta,
            repeat: bool=True):
        """
        Set a timer with the given step (time delta). The timer will run once
        the strategy is started. When the time delta is reached on_event() is
        passed the TimeEvent containing the timers unique label.
        Optionally the timer can be run repeatedly whilst the strategy is running.

        :param label: The unique label for the timer.
        :param step: The time delta step for the timer.
        :param repeat: The option for the timer to repeat until the strategy is stopped.
        """
        # TODO

    @typechecking
    def set_time_alert(
            self,
            label: Label,
            alert_time: datetime):
        """
        Set a time alert for the given time. When the time is reached and the
        strategy is running, on_event() is passed the TimeEvent containing the
        alerts unique label.

        :param label: The unique label for the alert.
        :param alert_time: The time for the alert.
        """
        # TODO

    def start(self):
        """
        Starts the trade strategy.
        """
        self._log(f"Starting...")
        self._is_running = True
        self.on_start()
        self._log(f"Running...")

    @typechecking
    def generate_order_id(self, symbol: Symbol) -> OrderId:
        """
        Generates a unique order identifier with the given symbol.

        :param symbol: The order symbol.
        :return: The unique order identifier.
        """
        return self._order_id_generator.generate(symbol)

    @typechecking
    def get_opposite_side(self, side: OrderSide) -> OrderSide:
        """
        Get the opposite order side from the original side given.

        :param side: The original order side.
        :return: The opposite order side.
        """
        return OrderSide.BUY if side is OrderSide.SELL else OrderSide.SELL

    @typechecking
    def get_flatten_side(self, market_position: MarketPosition) -> OrderSide:
        """
        Get the order side needed to flatten the position from the given market position.

        :param market_position: The market position to flatten.
        :return: The order side to flatten.
        """
        if market_position is MarketPosition.LONG:
            return OrderSide.SELL
        elif market_position is MarketPosition.SHORT:
            return OrderSide.BUY
        else:
            raise ValueError("Cannot flatten a FLAT position.")

    @typechecking
    def submit_order(self, order: Order):
        """
        Send a submit order request with the given order to the execution client.

        :param order: The order to submit.
        """
        # Preconditions
        if order.id in self._order_book:
            raise ValueError(
                "The order id is already contained in the order book (must be unique).")

        self._order_book[order.id] = order
        self._exec_client.submit_order(order, self._id)

    @typechecking
    def cancel_order(
            self,
            order: Order,
            cancel_reason: str=None):
        """
        Send a cancel order request for the given order to the execution client.

        :param order: The order to cancel.
        :param cancel_reason: The reason for cancellation (will be logged).
        """
        # Preconditions
        if order.id not in self._order_book.keys():
            raise ValueError("The order id was not found in the order book.")

        if cancel_reason is None:
            cancel_reason = 'NONE'
        self._exec_client.cancel_order(order, cancel_reason)

    @typechecking
    def modify_order(
            self,
            order: Order,
            new_price: Decimal):
        """
        Send a modify order request for the given order with the given new price
        to the execution client.

        :param order: The order to modify.
        :param new_price: The new price for the given order.
        """
        # Preconditions
        if order.id not in self._order_book.keys():
            raise ValueError("The order id was not found in the order book.")

        self._exec_client.modify_order(order, new_price)

    def stop(self):
        """
        Stops the trade strategy.
        """
        self._log(f"Stopping...")
        self.on_stop()
        self._is_running = False
        self._log(f"Stopped.")

    def reset(self):
        """
        Reset the trade strategy by clearing all stateful internal values and
        returning it to a fresh state (strategy must not be running).
        """
        if self._is_running:
            self._log(f"[Warning] Cannot reset a running strategy...")
            return

        self._ticks = {}  # type: Dict[Symbol, Tick]
        self._bars = {}   # type: Dict[BarType, List[Bar]]

        # Reset all indicators.
        for indicator_list in self._indicators.values():
            [indicator.reset() for indicator in indicator_list]

        self.on_reset()
        self._log(f"Reset.")

    # @typechecking: client checked in preconditions.
    def _register_execution_client(self, client):
        """
        Register the execution client with the strategy.

        :param client: The execution client to register.
        """
        # Preconditions
        if client is None:
            raise ValueError("The client cannot be None.")
        if client.__class__.__mro__[-2].__name__ != 'ExecutionClient':
            raise TypeError("The client must inherit from the ExecutionClient base class.")

        self._exec_client = client

    @typechecking
    def _update_ticks(self, tick: Tick):
        """"
        Updates the last held tick with the given tick then calls the on_tick
        method for the inheriting class.

        :param tick: The tick received.
        """
        # Update the internal ticks.
        self._ticks[tick.symbol] = tick

        # Calls on_tick() if the strategy is running.
        if self._is_running:
            self.on_tick(tick)

    @typechecking
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
        # Update the internal bars.
        if bar_type not in self._bars:
            self._bars[bar_type] = []  # type: List[Bar]
        self._bars[bar_type].insert(0, bar)

        # Update the internal indicators.
        if bar_type in self._indicators:
            self._update_indicators(bar_type, bar)

        # Calls on_bar() if the strategy is running.
        if self._is_running:
            self.on_bar(bar_type, bar)

    @typechecking
    def _update_indicators(
            self,
            bar_type: BarType,
            bar: Bar):
        """
        Updates the internal indicators of the given bar type with the given bar.

        :param bar_type: The bar type to update.
        :param bar: The bar for update.
        """
        # Preconditions checked in _update_bars.
        if bar_type not in self._indicators:
            # No indicators to update with this bar (remove this for production.)
            return

        # For each updater matching the given bar type -> update with the bar.
        [updater.update(bar) for updater in self._indicator_updaters[bar_type]]

    @typechecking
    def _update_events(self, event: Event):
        """
        Updates the strategy with the given event.

        :param event: The event received.
        """
        self._log(str(event))

        # Order events.
        if isinstance(event, OrderEvent):
            order_id = event.order_id
            if order_id in self._order_book:
                self._order_book[order_id].apply(event)
            else:
                self._log("Warning: The event order id not found in the order book.")

            # Position events.
            if isinstance(event, OrderFilled) or isinstance(event, OrderPartiallyFilled):
                if event.symbol not in self._positions:
                    opened_position = Position(
                        event.symbol,
                        order_id,
                        datetime.utcnow())
                    self._positions[event.symbol] = opened_position
                    self._log(f"Opened {opened_position}.")
                self._positions[event.symbol].apply(event)

                # If this order event exits the position then save to the database,
                # and remove from list.
                if self._positions[event.symbol].is_exited:
                    # TODO: Save to database.
                    closed_position = self._positions[event.symbol]
                    self._positions.pop(event.symbol)
                    self._log(f"Closed {closed_position}.")

        # Account Events
        if isinstance(event, AccountEvent):
            self._account.apply(event)

        # Calls on_event() if the strategy is running.
        if self._is_running:
            self.on_event(event)

    @typechecking
    def _add_order(self, order: Order):
        """
        Adds the given order to the order book (the order identifier must be unique).

        :param order: The order to add.
        """
        if order.id in self._order_book.keys():
            self._log(
                "[Warning]: The order id is already contained in the order book for the strategy.")
            return

        self._order_book[order.id] = order

    @typechecking
    def _log(self, message: str):
        """
        Logs the given message.

        :param message: The message to log.
        """
        print(f"{str(self)}: {message}")


class IndicatorUpdater:
    """
    Provides an adapter for updating an indicator with a bar. When instantiated
    with a live indicator update method, the updater will inspect the method and
    construct the required parameter list for updates.
    """

    @typechecking
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

    @typechecking
    def update(self, bar: Bar):
        """
        Passes the needed values from the given bar to the indicator update
        method as a list of arguments.

        :param bar: The update bar.
        """
        args = [bar.__getattribute__(param) for param in self._update_params]
        self._update_method(*args)
