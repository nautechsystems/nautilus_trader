#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_strategy.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import datetime
import pytz

from decimal import Decimal

from test_kit.objects import ObjectStorer
from test_kit.strategies import TestStrategy1
from inv_trader.enums import Venue, Resolution, QuoteType
from inv_trader.objects import Tick, BarType, Bar
from inv_trader.events import AccountEvent, OrderEvent, TimeEvent
from inv_trader.strategy import TradeStrategy
from inv_trader.strategy import IndicatorUpdater
from inv_indicators.average.ema import ExponentialMovingAverage
from inv_indicators.intrinsic_network import IntrinsicNetwork


class TradeStrategyTests(unittest.TestCase):

    def test_can_get_strategy_name(self):
        # Arrange
        strategy = TradeStrategy()

        # Act
        result = strategy.name

        # Assert
        self.assertEqual('TradeStrategy', result)

    def test_can_get_strategy_label(self):
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

    def test_strategy_equality(self):
        # Arrange
        strategy1 = TradeStrategy()
        strategy2 = TradeStrategy('AUDUSD-001')
        strategy3 = TradeStrategy('AUDUSD-002')

        # Act
        result1 = strategy1 == strategy1
        result2 = strategy1 == strategy2
        result3 = strategy2 == strategy3
        result4 = strategy1 != strategy1
        result5 = strategy1 != strategy2
        result6 = strategy2 != strategy3

        # Assert
        self.assertTrue(result1)
        self.assertFalse(result2)
        self.assertFalse(result3)
        self.assertFalse(result4)
        self.assertTrue(result5)
        self.assertTrue(result6)

    def test_strategy_is_hashable(self):
        # Arrange
        strategy = TradeStrategy('Test')

        # Act
        result = strategy.__hash__()

        # Assert
        # If this passes then result must be an int.
        self.assertTrue(result != 0)

    def test_strategy_str_and_repr(self):
        # Arrange
        strategy = TradeStrategy('GBPUSD-MM')

        # Act
        result1 = str(strategy)
        result2 = repr(strategy)

        # Assert
        self.assertEqual('TradeStrategy-GBPUSD-MM', result1)
        self.assertTrue(result2.startswith('<TradeStrategy-GBPUSD-MM object at'))
        self.assertTrue(result2.endswith('>'))

    def test_can_add_indicator_to_strategy(self):
        # Arrange
        storer = ObjectStorer()

        # Act
        strategy = TestStrategy1(storer)

        # Assert
        self.assertEqual(strategy.ema1, strategy.all_indicators[strategy.gbpusd_1sec_mid][0])
        self.assertEqual(strategy.ema2, strategy.all_indicators[strategy.gbpusd_1sec_mid][1])

    def test_indicator_labels_returns_expected_list(self):
        # Arrange
        storer = ObjectStorer()
        strategy = TestStrategy1(storer)

        # Act
        result = strategy.indicator_labels

        # Assert
        self.assertTrue('ema1' in result)
        self.assertTrue('ema2' in result)

    def test_can_start_strategy(self):
        # Arrange
        storer = ObjectStorer()
        strategy = TestStrategy1(storer)

        # Act
        strategy.start()

        # Assert
        self.assertTrue(strategy.is_running)
        self.assertTrue('custom start logic' in storer.get_store)

    def test_can_stop_strategy(self):
        # Arrange
        storer = ObjectStorer()
        strategy = TestStrategy1(storer)
        strategy.start()

        # Act
        strategy.stop()

        # Assert
        self.assertFalse(strategy.is_running)
        self.assertTrue('custom stop logic' in storer.get_store)

    def test_can_update_strategy_bars(self):
        # Arrange
        storer = ObjectStorer()
        strategy = TestStrategy1(storer)

        bar_type = BarType('gbpusd',
                           Venue.FXCM,
                           1,
                           Resolution.SECOND,
                           QuoteType.MID)

        bar = Bar(
            Decimal('1.00001'),
            Decimal('1.00004'),
            Decimal('1.00003'),
            Decimal('1.00002'),
            100000,
            datetime.datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC))

        # Act
        strategy._update_bars(bar_type, bar)

        # Assert
        self.assertFalse(strategy.is_running)
        self.assertEqual(1, len(strategy.all_bars[bar_type]))
        self.assertEqual(1, len(strategy.bars(bar_type)))
        self.assertEqual(1, strategy.ema1.count)
        self.assertEqual(1, strategy.ema2.count)
        self.assertEqual(0, len(storer.get_store))

    def test_can_reset_strategy(self):
        # Arrange
        storer = ObjectStorer()
        strategy = TestStrategy1(storer)

        bar_type = BarType('gbpusd',
                           Venue.FXCM,
                           1,
                           Resolution.SECOND,
                           QuoteType.MID)

        bar = Bar(
            Decimal('1.00001'),
            Decimal('1.00004'),
            Decimal('1.00003'),
            Decimal('1.00002'),
            100000,
            datetime.datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC))

        strategy._update_bars(bar_type, bar)

        # Act
        strategy.reset()

        # Assert
        self.assertFalse(strategy.is_running)
        self.assertEqual(0, strategy.ema1.count)
        self.assertEqual(0, strategy.ema2.count)
        self.assertTrue('custom reset logic' in storer.get_store)


class IndicatorUpdaterTests(unittest.TestCase):

    def test_can_update_ema_indicator(self):
        # Arrange
        ema = ExponentialMovingAverage(20)
        updater = IndicatorUpdater(ema.update)
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

    def test_can_update_intrinsic_networks_indicator(self):
        # Arrange
        intrinsic = IntrinsicNetwork(0.2, 0.2)
        updater = IndicatorUpdater(intrinsic.update_mid)
        bar = Bar(
            Decimal('1.00001'),
            Decimal('1.00004'),
            Decimal('1.00003'),
            Decimal('1.00002'),
            1000,
            datetime.datetime(1970, 1, 1, 0, 0, 0, 0, pytz.UTC))

        # Act
        updater.update(bar)
        result = intrinsic.state

        # Assert
        self.assertTrue(intrinsic.initialized)
        self.assertEqual(0, result)
