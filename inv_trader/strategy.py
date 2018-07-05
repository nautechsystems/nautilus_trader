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
import cython

from typing import List
from typing import Dict

from inv_trader.enums import Venue
from inv_trader.objects import Tick, BarType, Bar


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
        self._bars = Dict[BarType, List[Bar]]
        self._ticks = Dict[str, Tick]
        self._indicators = Dict[BarType, List[object]]
        self._indicator_labels = Dict[str, object]
        self._ind_updater_labels = Dict[BarType, List[str]]
        self._ind_updaters = Dict[str, IndicatorUpdater]

    def __str__(self) -> str:
        """
        :return: The str() string representation of the strategy.
        """
        return f"{self._name}:{self._label}"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the strategy.
        """
        return f"<{str(self)} object at {id(self)}>"

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
    def all_indicators(self) -> Dict[BarType, List[object]]:
        """
        :return: The internally held indicators dictionary for the strategy.
        """
        return self._indicators

    @property
    def all_bars(self) -> Dict[BarType, List[Bar]]:
        """
        :return: The internally held bars dictionary for the strategy.
        """
        return self._bars

    @property
    def all_ticks(self) -> Dict[str, Tick]:
        """
        :return: The internally held ticks dictionary for the strategy
        """
        return self._ticks

    def indicators(self, bar_type: BarType) -> List[object]:
        """
        Get the indicators list for the given bar type.

        :param: The bar type for the indicators list.
        :return: The internally held indicators for the given bar type.
        :raises: KeyError: If the indicators dictionary does not contain the bar type.
        """
        if bar_type not in self._indicators:
            raise KeyError(f"The indicators dictionary does not contain {bar_type}.")

        return self._indicators[bar_type]

    def indicator(self, label: str) -> object:
        """
        Get the indicator for the given unique label.

        :param: label: The unique label for the indicator.
        :return: The internally held indicator for the given unique label.
        :raises: KeyError: If the indicator labels dictionary does not contain the bar type.
        """
        if label not in self._indicator_labels:
            raise KeyError(f"The indicator dictionary does not contain the label {label}.")

        return self._indicator_labels[label]

    def bars(self, bar_type: BarType) -> List[Bar]:
        """
        Get the bars for the given bar type.

        :param bar_type: The bar type to get.
        :return: The list of bars.
        :raises: KeyError: If the bars dictionary does not contain the bar type.
        """
        if bar_type not in self._bars:
            raise KeyError(f"The bars dictionary does not contain {bar_type}.")

        return self._bars[bar_type]

    def bar(
            self,
            bar_type: BarType,
            index: int) -> Bar:
        """
        Get the bar for the given bar type at index (reverse indexed, 0 is last bar).

        :param bar_type: The bar type to get.
        :param index: The index to get.
        :return: The found bar.
        :raises: KeyError: If the bars dictionary does not contain the bar type.
        :raises: ValueError: If the index is negative.
        """
        if bar_type not in self._bars:
            raise KeyError(f"The bars dictionary does not contain {bar_type}.")
        if index < 0:
            raise ValueError("The index cannot be negative.")

        return self._bars[bar_type][index]

    def last_tick(
            self,
            symbol: str,
            venue: Venue) -> Tick:
        """
        Get the last tick held for the given parameters.

        :param symbol: The last tick symbol.
        :param venue: The last tick venue.
        :return: The tick object.
        :raises: KeyError: If the ticks dictionary does not contain the symbol and venue string.
        """
        key = f'{symbol.lower()}.{venue.name.lower()}'
        if key not in self._ticks:
            raise KeyError(f"The ticks dictionary does not contain {key}.")

        return self._ticks[key]

    def set_indicator(
            self,
            bar_type: BarType,
            indicator: object,
            update_method: callable,
            label: str):
        """
        Set the given indicator to receive bars of the given bar type.
        The indicator must be from the inv_indicators package.

        :param bar_type: The indicators bar type.
        :param indicator: The indicator to set.
        :param update_method: The update method for the indicator.
        :param label: The unique label for this indicator.
        :raises: ValueError: If any argument is None.
        :raises: KeyError: If the label is not unique.
        """
        if bar_type is None:
            raise ValueError("The bar type cannot be None.")
        if indicator is None:
            raise ValueError("The indicator cannot be None.")
        if update_method is None:
            raise ValueError("The update_method cannot be None.")
        if label is None:
            raise ValueError("The label cannot be None.")
        if label in self._indicator_labels:
            raise KeyError("The label is not unique (already contained in the indicator labels).")

        if bar_type not in self._indicators:
            self._indicators[bar_type] = List[object]

        self._indicators[bar_type].append(indicator)
        self._ind_updaters[label] = IndicatorUpdater(indicator, update_method)

        # TODO: Refactor these separate labels lists.
        self._indicator_labels[label] = indicator

        if bar_type not in self._ind_updater_labels:
            self._ind_updater_labels[bar_type] = List[IndicatorUpdater]
        self._ind_updater_labels[bar_type].append(label)

    def set_timer(
            self,
            label: str,
            step: datetime.timedelta,
            handler: callable,
            repeat: bool=True):
        """
        Set a timer with the given step (time delta). The handler is called
        with a string containing the timers unique label and current time.

        :param label: The unique label for the timer.
        :param step: The time delta step for the timer.
        :param handler: The handler to be called.
        :param repeat: The option for the timer to repeat until the strategy is stopped.
        """
        # TODO

    def set_alarm(
            self,
            label: str,
            time: datetime.datetime,
            handler: callable):
        """
        Set an alarm for the given time. When the time is reached, the handler
        is called with a string containing the alarms unique label and the
        current time.

        :param label: The unique label for the alarm.
        :param time: The time for the alarm.
        :param handler: The handler to be called.
        """
        # TODO

    @abc.abstractmethod
    def on_start(self):
        # Raise exception if not overridden in implementation.
        raise NotImplementedError

    @abc.abstractmethod
    def on_tick(self, tick: Tick):
        # Raise exception if not overridden in implementation.
        raise NotImplementedError

    @abc.abstractmethod
    def on_bar(self, bar_type: BarType, bar: Bar):
        # Raise exception if not overridden in implementation.
        raise NotImplementedError

    @abc.abstractmethod
    def on_account(self, message):
        # Raise exception if not overridden in implementation.
        raise NotImplementedError

    @abc.abstractmethod
    def on_message(self, message):
        # Raise exception if not overridden in implementation.
        raise NotImplementedError

    @abc.abstractmethod
    def on_stop(self):
        # Raise exception if not overridden in implementation.
        raise NotImplementedError

    def start(self):
        """
        Starts the trade strategy.
        """
        self._log(f"Starting {self.name}...")
        self._is_running = True
        self.on_start()

    def stop(self):
        """
        Stops the trade strategy.
        """
        self._log(f"Stopping {self.name}...")
        self._is_running = False
        self.on_stop()

    def reset(self):
        """
        Reset the trade strategy by clearing all stateful internal values and
        returning it to a fresh state (strategy must not be running).
        """
        if self._is_running:
            self._log(f"{self.name} Warning: Cannot reset a running strategy...")
            return

        self._ticks = {}
        self._bars = {}
        self._log(f"Reset {self.name}.")

    def _update_tick(self, tick: Tick):
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
            self._log(f"{self.name} Warning: _update_tick() was given an invalid Tick.")
            return

        # Update the internal ticks.
        key = f'{tick.symbol}.{tick.venue.name.lower()}'
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
            self._log(f"{self.name} Warning: _update_bar() was given an invalid BarType.")
            return
        if bar is None:
            self._log("{self.name} Warning: _update_bar() was given None.")
            return
        if not isinstance(bar_type, Bar):
            self._log(f"{self.name} Warning: _update_bar() was given an invalid Bar.")

        # Update the internal bars.
        if bar_type not in self._bars:
            self._bars[bar_type] = List[Bar]
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
            # No indicators to update with this bar.
            # Remove this for production.
            return

        # For each updater matching the given bar type -> update with the bar.
        [updater.update(bar) for updater in self._ind_updater_labels[bar_type]]

    # Avoid making a static method for now.
    def _log(self, message: str):
        """
        Logs the given message.

        :param message: The message to log.
        """
        print(message)


class IndicatorUpdater:
    """
    Provides an adapter for updating an indicator with a bar. When instantiated
    with a live indicator object and update method the updater will inspect
    the update method and construct the required parameter list for updates.
    """

    def __init__(
            self,
            indicator: object,
            update_method: classmethod):
        """
        Initializes a new instance of the IndicatorUpdater class.

        :param indicator: The indicator for the updater.
        :param update_method: The indicators update method.
        """
        self._name = indicator.name
        self._update_method = update_method
        self._params = []



    def update(self, bar: Bar):
        """
        Passes the correct parameters from the given bar to the indicator update method.

        :param bar: The bar to update with.
        """
        # Guard clause (design time).
        assert isinstance(bar, Bar)

        update_params = []

        for param in self._params:
            if param is 'point':
                update_params.append(float(bar.close))
            elif param is 'price':
                update_params.append(float(bar.close))
            elif param is 'mid':
                update_params.append(float(bar.close))
            elif param is 'open':
                update_params.append(float(bar.open))
            elif param is 'high':
                update_params.append(float(bar.high))
            elif param is 'low':
                update_params.append(float(bar.low))
            elif param is 'close':
                update_params.append(float(bar.close))
            elif param is 'timestamp':
                update_params.append(bar.timestamp)

        self._update_method(*update_params)
