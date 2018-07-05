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
import inv_indicators

from decimal import Decimal
from typing import List

from test_kit.objects import ObjectStorer
from test_kit.strategies import TestStrategy1
from inv_trader.objects import Tick, BarType, Bar
from inv_trader.enums import Venue, Resolution, QuoteType
from inv_trader.strategy import TradeStrategy
from inv_trader.strategy import IndicatorUpdater
from inv_indicators.average.ema import ExponentialMovingAverage


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

    def test_can_add_indicator_to_strategy(self):
        # Arrange
        storer = ObjectStorer()
        strategy = TestStrategy1(storer)

        # Act
        # Assert
        print(strategy.all_indicators[strategy.gbpusd_1sec_mid])
        self.assertEqual(strategy.ema1, strategy.all_indicators[strategy.gbpusd_1sec_mid][0])


class IndicatorUpdaterTests(unittest.TestCase):

    def test_can_update_ema_indicator(self):
        # Arrange
        ema = ExponentialMovingAverage(20)
        updater = IndicatorUpdater(ema, ema.update)
        bar = Bar(
            Decimal('1.00001'),
            Decimal('1.00004'),
            Decimal('1.00003'),
            Decimal('1.00002'),
            1000,
            datetime.datetime(1970, 1, 1, 0, 0, 0, 0, pytz.UTC))

        # Act
        updater.update(bar)
        result = ema.value

        # Assert
        self.assertEqual(1.00002, result)
