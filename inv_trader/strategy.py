#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="strategy.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import abc

from typing import List
from typing import Dict

from inv_trader.enums import Venue
from inv_trader.objects import Tick, BarType, Bar


class TradeStrategy(object):
    """
    The base class for all trade strategies.
    """

    __metaclass__ = abc.ABCMeta

    def __init__(self):
        """
        Initializes a new instance of the abstract TradeStrategy class.
        """
        self._is_running = False
        self._name = self.__class__.__name__
        self._bars = {}
        self._last_ticks = {}

    def __str__(self) -> str:
        """
        :return: The str() string representation of the trade strategy.
        """
        return f"{self._name}"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the trade strategy.
        """
        return f"<{str(self)} object at {id(self)}>"

    @property
    def is_running(self) -> bool:
        """
        :return: The name of the trade strategy.
        """
        return self._is_running

    @property
    def name(self) -> str:
        """
        :return: The name of the trade strategy.
        """
        return self._name

    @property
    def bars(self) -> Dict[BarType, List[Bar]]:
        """
        :return: The internally held bars for the strategy.
        """
        return self._bars

    def last_tick(
            self,
            symbol: str,
            venue: Venue) -> Tick:
        """
        Get the last tick held for the given parameters.

        :param symbol: The last tick symbol.
        :param: venue: The last tick venue.
        :return: The tick object.
        """
        key = f'{symbol.lower()}.{venue.name.lower()}'
        if key not in self._last_ticks:
            raise KeyError(f"The last ticks does not contain {key}.")

        return self._last_ticks[key]

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

        self._last_ticks = {}
        self._bars = {}
        self._log(f"Reset {self.name}.")

    def _update_tick(self, tick: Tick):
        """"
        Updates the last held tick with the given tick then calls the on_tick
        method for the super class.
        """
        if tick is None:
            self._log(f"{self.name} Warning: update_tick() was given None.")
            return

        key = f'{tick.symbol}.{tick.venue.name.lower()}'
        self._last_ticks[key] = tick
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
        if bar_type is None:
            self._log(f"{self.name} Warning: update_bar() was given bar_type of None.")
            return
        if bar is None:
            self._log("{self.name} Warning: update_bar() was given bar of None.")
            return
        if bar_type not in self._bars:
            self._bars[bar_type] = []

        self._bars[bar_type].append(bar)
        self.on_bar(bar_type, bar)

    # Avoid making a static method for now.
    def _log(self, message: str):
        """
        Logs the given message.

        :param message: The message to log.
        """
        print(message)
