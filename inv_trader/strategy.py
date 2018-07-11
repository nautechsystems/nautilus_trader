#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="strategy.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import abc
import datetime
import inspect

from typing import List
from typing import Dict
from typing import KeysView

from inv_trader.model.enums import Venue
from inv_trader.model.objects import Tick, BarType, Bar
from inv_trader.model.order import Order
from inv_trader.model.events import Event, OrderEvent

Label = str
Symbol = str
Indicator = object
OrderId = str


class TradeStrategy:
    """
    The abstract base class for all trade strategies.
    """

    __metaclass__ = abc.ABCMeta

    def __init__(self, label: str=None):
        """
        Initializes a new instance of the TradeStrategy abstract class.

        :param: label: The unique label for the strategy (can be None).
        """
        if label is None:
            label = ''

        self._name = self.__class__.__name__
        self._label = label
        self._is_running = False
        self._ticks = {}               # type: Dict[Symbol, Tick]
        self._bars = {}                # type: Dict[BarType, List[Bar]]
        self._indicators = {}          # type: Dict[BarType, List[Indicator]]
        self._indicator_updaters = {}  # type: Dict[BarType, List[IndicatorUpdater]]
        self._indicator_index = {}     # type: Dict[Label, Indicator]
        self._order_book = {}          # type: Dict[OrderId, Order]
        self._exec_client = None

        self._log(f"{self} initialized.")

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
        :return: The unique label of the strategy.
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

    def last_tick(
            self,
            symbol: Symbol,
            venue: Venue) -> Tick:
        """
        Get the last tick held for the given parameters.

        :param symbol: The last tick symbol.
        :param venue: The last tick venue.
        :return: The tick object.
        :raises: KeyError: If the strategy does not contain a tick for the symbol and venue string.
        """
        # Preconditions
        key = f'{symbol.lower()}.{venue.name.lower()}'
        if key not in self._ticks:
            raise KeyError(f"The ticks dictionary does not contain {key}.")

        return self._ticks[key]

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

    def add_indicator(
            self,
            bar_type: BarType,
            indicator: Indicator,
            update_method: callable,
            label: Label):
        """
        Add the given indicator to the strategy. It will receive bars of the
        given bar type. The indicator must be from the inv_indicators package.

        :param bar_type: The indicators bar type.
        :param indicator: The indicator to set.
        :param update_method: The update method for the indicator.
        :param label: The unique label for this indicator.
        :raises: ValueError: If any argument is None.
        :raises: TypeError: If the indicator is not a type of Indicator.
        :raises: TypeError: If the update method is not callable.
        :raises: KeyError: If the label is not unique.
        """
        # Preconditions
        if bar_type is None:
            raise ValueError("The bar type cannot be None.")
        if indicator is None:
            raise ValueError("The indicator cannot be None.")
        if indicator.__class__.__mro__[-2].__name__ != 'Indicator':
            raise TypeError("The indicator must inherit from the Indicator base class.")
        if update_method is None:
            raise ValueError("The update_method cannot be None.")
        if update_method is not None and not callable(update_method):
            raise TypeError("The update_method must be a callable object.")
        if label is None:
            raise ValueError("The label cannot be None.")
        if label in self._indicator_index.keys():
            raise KeyError("The indicator label must be unique for this strategy.")

        if bar_type not in self._indicators:
            self._indicators[bar_type] = []  # type: List[Indicator]
        self._indicators[bar_type].append(indicator)

        if bar_type not in self._indicator_updaters:
            self._indicator_updaters[bar_type] = []  # type: List[IndicatorUpdater]
        self._indicator_updaters[bar_type].append(IndicatorUpdater(update_method))

        self._indicator_index[label] = indicator

    def set_timer(
            self,
            label: Label,
            step: datetime.timedelta,
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

    def set_time_alert(
            self,
            label: Label,
            alert_time: datetime.datetime):
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
        self._log(f"{str(self)} starting...")
        self._is_running = True
        self.on_start()
        self._log(f"{str(self)} running...")

    def submit_order(self, order: Order):
        """
        Submit the order to the execution client.
        :return:
        """
        # Preconditions
        if order.id in self._order_book:
            raise ValueError("The order id is already contained in the order book (must be unique).")

        self._order_book[order.id] = order
        self._exec_client.submit_order(order, str(self))

    def stop(self):
        """
        Stops the trade strategy.
        """
        self._log(f"{str(self)} stopping...")
        self._is_running = False
        self.on_stop()
        self._log(f"{str(self)} stopped.")

    def reset(self):
        """
        Reset the trade strategy by clearing all stateful internal values and
        returning it to a fresh state (strategy must not be running).
        """
        if self._is_running:
            self._log(f"{str(self)} Warning: Cannot reset a running strategy...")
            return

        self._ticks = {}  # type: Dict[Symbol, Tick]
        self._bars = {}   # type: Dict[BarType, List[Bar]]

        # Reset all indicators.
        for indicator_list in self._indicators.values():
            [indicator.reset() for indicator in indicator_list]

        self.on_reset()
        self._log(f"{str(self)} reset.")

    def _register_execution_client(self, client):
        """
        Register the execution client with the strategy.

        :param client: The execution client to register.
        """
        self._exec_client = client

    def _update_ticks(self, tick: Tick):
        """"
        Updates the last held tick with the given tick then calls the on_tick
        method for the inheriting class.

        :param tick: The tick received.
        """
        # Guard clauses to catch design errors and make code robust in production.
        # Warnings are logged.
        if tick is None:
            self._log(f"{str(self)} Warning: update_tick() was given None.")
            return
        if not isinstance(tick, Tick):
            self._log(f"{str(self)} Warning: _update_tick() was given an invalid Tick {tick}.")
            return

        # Update the internal ticks.
        key = f'{tick.symbol.lower()}.{tick.venue.name.lower()}'
        self._ticks[key] = tick

        # Calls on_tick() if the strategy is running.
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
        # Guard clauses to catch design errors and make code robust in production.
        # Warnings are logged.
        if bar_type is None:
            self._log(f"{str(self)} Warning: _update_bar() was given None.")
            return
        if not isinstance(bar_type, BarType):
            self._log(f"{str(self)} Warning: _update_bar() was given an invalid BarType {bar_type}.")
            return
        if bar is None:
            self._log("{self.name} Warning: _update_bar() was given None.")
            return
        if not isinstance(bar, Bar):
            self._log(f"{str(self)} Warning: _update_bar() was given an invalid Bar {bar}.")

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
            # No indicators to update with this bar (remove this for production.)
            return

        # For each updater matching the given bar type -> update with the bar.
        [updater.update(bar) for updater in self._indicator_updaters[bar_type]]

    def _update_events(self, event: Event):
        """
        Updates the strategy with the given event.

        :param event: The event received.
        """
        # Preconditions
        if not isinstance(event, Event):
            raise TypeError(f"The event must be of type Event (was {type(event)}).")

        # Apply order event if order id contained in the order book.
        if isinstance(event, OrderEvent):
            order_id = event.order_id
            if order_id not in self._order_book.keys():
                raise ValueError("The given event order id was not in the order book.")
            self._order_book[order_id].apply(event)

        # Calls on_event() if the strategy is running.
        if self._is_running:
            self.on_event(event)

    def _add_order(self, order: Order):
        """
        Adds the given order to the order book (the order id must be unique).

        :param order: The order to add.
        """
        if order.id in self._order_book.keys():
            raise ValueError("The order id is already contained in the order book for the strategy.")

        self._order_book[order.id] = order

    def _log(self, message: str):
        """
        Logs the given message.

        :param message: The message to log.
        """
        print(message)


class IndicatorUpdater:
    """
    Provides an adapter for updating an indicator with a bar. When instantiated
    with a live indicator update method, the updater will inspect the method and
    construct the required parameter list for updates.
    """

    def __init__(self, update_method: callable):
        """
        Initializes a new instance of the IndicatorUpdater class.

        :param update_method: The indicators update method.
        """
        # Preconditions
        if update_method is None:
            raise ValueError("The update_method cannot be None.")
        if update_method is not None and not callable(update_method):
            raise TypeError("The update_method must be a callable object.")

        self._update_method = update_method
        self._update_params = []

        param_map = {
            'point': 'close',
            'price': 'close',
            'mid': 'close',
            'open': 'open',
            'high': 'high',
            'low': 'low',
            'close': 'close',
            'timestamp': 'timestamp'
        }

        for param in list(inspect.signature(update_method).parameters.keys()):
            self._update_params.append(param_map[param])

    def update(self, bar: Bar):
        """
        Passes the needed values from the given bar to the indicator update
        method as a list of arguments.

        :param bar: The update bar.
        """
        args = [bar.__getattribute__(param) for param in self._update_params]
        self._update_method(*args)
