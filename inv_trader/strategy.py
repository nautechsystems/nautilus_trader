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
        self._bars = []
        self._last_tick = None

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
    def bars(self) -> List[Bar]:
        """
        :return: The internally held bars for the strategy.
        """
        return self._bars

    @property
    def last_tick(self) -> Tick:
        """
        :return: The last tick the strategy.
        """
        return self._last_tick

    @abc.abstractmethod
    def reset(self):
        """
        Reset the trade strategy by clearing all stateful internal values and
        returning it to a fresh tate.
        """
        # do nothing yet

    def update_tick(self, tick: Tick):
        """"
        Updates the last held tick with the given tick then calls the on_tick
        method for the super class.
        """
        self._last_tick = tick
        super(TradeStrategy, self).on_tick()

    def update_bars(self, bar: Bar):
        """"
        Updates the internal list of bars with the given bar, then calls the
        on_bar method for the super class.
        """
        self._bars.append(bar)
        super(TradeStrategy, self).on_bar()

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
