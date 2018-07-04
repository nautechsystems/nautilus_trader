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
from inv_trader.objects import Tick, Bar


class TradeStrategy(object):
    """
    The base class for all trade strategies.
    """

    __metaclass__ = abc.ABCMeta

    def __init__(self, params: []=None):
        """
        Initializes a new instance of the abstract TradeStrategy class.

        :param params: The initialization parameters for the trade strategy.
        """
        self._name = self.__class__.__name__
        self._params = str(params)[1:-1].replace("'", "").strip('()')
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
    def name(self) -> str:
        """
        :return: The name of the trade strategy.
        """
        return self._name

    @property
    def bars(self) -> Dict[str, List[Bar]]:
        """
        :return: The internally held bars for the strategy.
        """
        return self._bars

    def last_tick(
            self,
            symbol: str,
            venue: Venue) -> Tick:
        """
        :param symbol: The last tick symbol.
        :return: venue: The last tick venue.
        """
        key = f'{symbol}.{venue.name.lower()}'
        if key not in self._last_ticks:
            raise KeyError(f"The last ticks do not contain {key}.")

        return self._last_ticks[key]

    @abc.abstractmethod
    def on_tick(self):
        # Raise exception if not overridden in implementation.
        raise NotImplementedError

    @abc.abstractmethod
    def on_bar(self):
        # Raise exception if not overridden in implementation.
        raise NotImplementedError

    @abc.abstractmethod
    def on_message(self):
        # Raise exception if not overridden in implementation.
        raise NotImplementedError

    def reset(self):
        """
        Reset the trade strategy by clearing all stateful internal values and
        returning it to a fresh tate.
        """
        self._last_ticks = {}
        self._bars = {}

    def _update_tick(self, tick: Tick):
        """"
        Updates the last held tick with the given tick then calls the on_tick
        method for the super class.
        """
        if tick is None:
            print("update_tick() was given None.")
            return

        key = f'{tick.symbol}.{tick.venue.name.lower()}'
        self._last_ticks[key] = tick
        self.on_tick()

    def _update_bars(
            self,
            bar_type: str,
            bar: Bar):
        """"
        Updates the internal dictionary of bars with the given bar, then calls the
        on_bar method for the super class.

        :param bar_type: The received bar type.
        :param bar: The received bar.
        """
        if bar_type is None:
            print("update_bar() was given bar_type of None.")
            return
        if bar is None:
            print("update_bar() was given bar of None.")
            return
        if bar_type not in self._bars:
            self._bars[bar_type] = []

        self._bars[bar_type].append(bar)
        self.on_bar()
