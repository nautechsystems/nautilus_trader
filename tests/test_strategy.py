#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_strategy.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import redis
import datetime
import pytz
import time

from decimal import Decimal
from typing import List


from inv_trader.strategy import TradeStrategy
from inv_trader.objects import Tick, BarType, Bar
from inv_trader.enums import Venue, Resolution, QuoteType


class TradeStrategyTests(unittest.TestCase):

    def test_can_get_strategy_name(self):
        # Arrange
        strategy = TradeStrategy()

        # Act
        result = strategy.name

        # Assert
        self.assertEqual('TradeStrategy', result)

    def test_label_for_strategy(self):
        # Arrange
        strategy1 = TradeStrategy()
        strategy2 = TradeStrategy(None)  # Simulating user ignoring type hint.
        strategy3 = TradeStrategy('EURUSD-Scalper')

        # Act
        result1 = strategy1.label
        result2 = strategy2.label
        result3 = strategy3.label

        # Assert
        self.assertEqual('', result1)
        self.assertEqual('', result2)
        self.assertEqual('EURUSD-Scalper', result3)

    def test_strategy_str_and_repr(self):
        # Arrange
        strategy = TradeStrategy('GBPUSD-MM')

        # Act
        result1 = str(strategy)
        result2 = repr(strategy)

        # Assert
        self.assertEqual('TradeStrategy:GBPUSD-MM', result1)
        self.assertTrue(result2.startswith('<TradeStrategy:GBPUSD-MM object at'))
        self.assertTrue(result2.endswith('>'))

