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

from inv_trader.enums import Venue
from inv_trader.objects import Tick, BarType, Bar
from inv_trader.events import AccountEvent, OrderEvent, ExecutionEvent, TimeEvent

Label = str
Symbol = str
Indicator = object


class TradeStrategy:
    """
    The base class for all trade strategies.
    """

    __metaclass__ = abc.ABCMeta

    def __init__(self, label: str=''):
        """
        Initializes a new instance of the TradeStrategy abstract class.

        :param: label: The unique label for the strategy (can be empty).
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

        self._log(f"{self} initialized.")

    def __eq__(self, other) -> bool:
        """
        Override the default equality comparison.
        """
        if isinstance(other, self.__class__):
            return self.__dict__ == other.__dict__
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
    def on_account_event(self, event: AccountEvent):
        """
        Called when an account event is received by the strategy.

        :param event: The account event received.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the strategy (or just add pass).")

    @abc.abstractmethod
    def on_order_event(self, event: OrderEvent):
        """
        Called when an order event is received by the strategy.

        :param event: The order event received.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the strategy (or just add pass).")

    @abc.abstractmethod
    def on_execution_event(self, event: ExecutionEvent):
        """
        Called when an execution event is received by the strategy.

        :param event: The execution event received.
        """
        # Raise exception if not overridden in implementation.
        raise NotImplementedError("Method must be implemented in the strategy (or just add pass).")

    @abc.abstractmethod
    def on_time_event(self, event: TimeEvent):
        """
        Called when a time event is received by the strategy.

        :param event: The time event received.
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

    def indicators(self, bar_type: BarType) -> List[Indicator]:
        """
        Get the indicators list for the given bar type.

        :param: The bar type for the indicators list.
        :return: The internally held indicators for the given bar type.
        :raises: KeyError: If the indicators dictionary does not contain the bar type.
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
        :raises: KeyError: If the indicator labels dictionary does not contain the bar type.
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
        :raises: KeyError: If the bars dictionary does not contain the bar type.
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
        :raises: KeyError: If the bars dictionary does not contain the bar type.
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
        :raises: KeyError: If the ticks dictionary does not contain the symbol and venue string.
        """
        # Preconditions
        key = f'{symbol.lower()}.{venue.name.lower()}'
        if key not in self._ticks:
            raise KeyError(f"The ticks dictionary does not contain {key}.")

        return self._ticks[key]

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
        the strategy is started. When the time delta is reached the
        on_time_event() is passed the TimeEvent containing the timers label.
        Optionally this can be repeated whilst the strategy is running.

        :param label: The label for the timer.
        :param step: The time delta step for the timer.
        :param repeat: The option for the timer to repeat until the strategy is stopped.
        """
        # TODO

    def set_time_alert(
            self,
            label: Label,
            time: datetime.datetime):
        """
        Set a time alert for the given time. When the time is reached and the
        strategy is running, the on_time_event() is passed the TimeEvent
        containing the alerts label.

        :param label: The unique label for the alarm.
        :param time: The time for the alarm.
        """
        # TODO

    def start(self):
        """
        Starts the trade strategy.
        """
        self._log(f"{self.name} starting...")
        self._is_running = True
        self.on_start()

    def stop(self):
        """
        Stops the trade strategy.
        """
        self._log(f"{self.name} stopping...")
        self._is_running = False
        self.on_stop()
        self._log(f"{self.name} stopped.")

    def reset(self):
        """
        Reset the trade strategy by clearing all stateful internal values and
        returning it to a fresh state (strategy must not be running).
        """
        if self._is_running:
            self._log(f"{self.name} Warning: Cannot reset a running strategy...")
            return

        self._ticks = {}  # type: Dict[Symbol, Tick]
        self._bars = {}   # type: Dict[BarType, List[Bar]]

        for indicator_list in self._indicators.values():
            [indicator.reset() for indicator in indicator_list]

        self._log(f"{self.name} reset.")

    def _update_ticks(self, tick: Tick):
        """"
        Updates the last held tick with the given tick then calls the on_tick
        method for the inheriting class.

        :param tick: The received tick.
        """
        # Guard clauses to catch design errors and make code robust in production.
        # Warnings are logged.
        if tick is None:
            self._log(f"{self.name} Warning: update_tick() was given None.")
            return
        if not isinstance(tick, Tick):
            self._log(f"{self.name} Warning: _update_tick() was given an invalid Tick {tick}.")
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

        :param bar_type: The received bar type.
        :param bar: The received bar.
        """
        # Guard clauses to catch design errors and make code robust in production.
        # Warnings are logged.
        if bar_type is None:
            self._log(f"{self.name} Warning: _update_bar() was given None.")
            return
        if not isinstance(bar_type, BarType):
            self._log(f"{self.name} Warning: _update_bar() was given an invalid BarType {bar_type}.")
            return
        if bar is None:
            self._log("{self.name} Warning: _update_bar() was given None.")
            return
        if not isinstance(bar, Bar):
            self._log(f"{self.name} Warning: _update_bar() was given an invalid Bar {bar}.")

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
        # Guard clauses (design time checking).
        assert bar_type is not None
        assert isinstance(bar_type, BarType)
        assert bar is not None
        assert isinstance(bar, Bar)

        if bar_type not in self._indicators:
            # No indicators to update with this bar (remove this for production.)
            return

        # For each updater matching the given bar type -> update with the bar.
        [updater.update(bar) for updater in self._indicator_updaters[bar_type]]

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
        # Guard clause (design time).
        assert bar is not None
        assert isinstance(bar, Bar)

        args = [bar.__getattribute__(param) for param in self._update_params]
        self._update_method(*args)
