#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="trader.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False

import uuid

from inv_trader.core.precondition cimport Precondition
from inv_trader.common.clock cimport LiveClock
from inv_trader.common.logger cimport Logger, LoggerAdapter
from inv_trader.strategy cimport TradeStrategy


cdef class Trader:
    """
    Represents a trader for managing a portfolio of trade strategies.
    """

    def __init__(self,
                 str name='NONE',
                 list strategies=[],
                 Clock clock=LiveClock(),
                 Logger logger=None):
        """
        Initializes a new instance of the Trader class.

        :param name: The name of the trader.
        :param strategies: The initial list of strategies to manage (cannot be empty).
        :param logger: The logger for the trader (can be None).
        """
        Precondition.valid_string(name, 'name')
        Precondition.list_type(strategies, TradeStrategy, 'strategies')
        Precondition.not_none(clock, 'clock')

        self._clock = clock
        if logger is None:
            self.log = LoggerAdapter(f"{self.__class__.__name__}-{self.name}")
        else:
            self.log = LoggerAdapter(f"{self.__class__.__name__}-{self.name}", logger)
        self.name = Label(name)
        self.id = GUID(uuid.uuid4())
        self.strategies = strategies
        self.started_datetimes = []
        self.stopped_datetimes = []
