#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_account.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from decimal import Decimal

from inv_trader.analyzers import SpreadAnalyzer, LiquidityAnalyzer
from inv_trader.model.enums import Venue
from inv_trader.model.objects import Symbol, Price, Tick
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = Symbol('AUDUSD', Venue.FXCM)


class SpreadAnalyzerTests(unittest.TestCase):

    def test_can_calculate_metrics_with_no_updates(self):
        # Arrange
        analyzer = SpreadAnalyzer(decimal_precision=5)

        # Act
        analyzer.calculate_metrics()

        # Assert
        self.assertFalse(analyzer.initialized)
        self.assertEqual([0.0], analyzer.get_average_spreads())
        self.assertEqual(Decimal(0), analyzer.current_spread)
        self.assertEqual(Decimal(0), analyzer.average_spread)
        self.assertEqual(Decimal(0), analyzer.maximum_spread)
        self.assertEqual(Decimal(0), analyzer.minimum_spread)
        self.assertEqual(Decimal, type(analyzer.average_spread))
        self.assertEqual(Decimal, type(analyzer.get_average_spreads()[0]))

    def test_can_update_with_ticks(self):
        # Arrange
        analyzer = SpreadAnalyzer(decimal_precision=5)

        # Act
        analyzer.update(Tick(AUDUSD_FXCM, Price('0.89000'), Price('0.89001'), UNIX_EPOCH))
        analyzer.update(Tick(AUDUSD_FXCM, Price('0.89000'), Price('0.89002'), UNIX_EPOCH))

        # Assert
        self.assertFalse(analyzer.initialized)
        self.assertEqual(Decimal('0.00002'), analyzer.current_spread)
        self.assertEqual(Decimal('0.00002'), analyzer.average_spread)
        self.assertEqual(Decimal('0.00002'), analyzer.maximum_spread)
        self.assertEqual(Decimal('0.00001'), analyzer.minimum_spread)

    def test_can_reset(self):
        # Arrange
        analyzer = SpreadAnalyzer(decimal_precision=5)
        analyzer.update(Tick(AUDUSD_FXCM, Price('0.89000'), Price('0.89001'), UNIX_EPOCH))
        analyzer.update(Tick(AUDUSD_FXCM, Price('0.89000'), Price('0.89002'), UNIX_EPOCH))

        analyzer.calculate_metrics()

        # Act
        analyzer.reset()

        # Assert
        self.assertFalse(analyzer.initialized)
        self.assertEqual(Decimal(0), analyzer.current_spread)
        self.assertEqual(Decimal(0), analyzer.average_spread)
        self.assertEqual(Decimal(0), analyzer.maximum_spread)
        self.assertEqual(Decimal(0), analyzer.minimum_spread)
        self.assertEqual([], analyzer.get_average_spreads())

    def test_can_calculate_and_set_metrics(self):
        # Arrange
        analyzer = SpreadAnalyzer(decimal_precision=5)

        analyzer.update(Tick(AUDUSD_FXCM, Price('0.89000'), Price('0.89001'), UNIX_EPOCH))
        analyzer.update(Tick(AUDUSD_FXCM, Price('0.89000'), Price('0.89002'), UNIX_EPOCH))
        analyzer.update(Tick(AUDUSD_FXCM, Price('0.89001'), Price('0.89004'), UNIX_EPOCH))
        analyzer.update(Tick(AUDUSD_FXCM, Price('0.89001'), Price('0.89001'), UNIX_EPOCH))
        analyzer.update(Tick(AUDUSD_FXCM, Price('0.89002'), Price('0.89001'), UNIX_EPOCH))
        analyzer.update(Tick(AUDUSD_FXCM, Price('0.89001'), Price('0.89001'), UNIX_EPOCH))

        first_average = analyzer.average_spread

        # Act
        analyzer.calculate_metrics()

        # Assert
        self.assertTrue(analyzer.initialized)
        self.assertEqual(Decimal('0.00001'), first_average)
        self.assertEqual(Decimal('0.00000'), analyzer.current_spread)
        self.assertEqual(Decimal('0.00001'), analyzer.average_spread)
        self.assertEqual(Decimal('0.00003'), analyzer.maximum_spread)
        self.assertEqual(Decimal('-0.00001'), analyzer.minimum_spread)
        self.assertTrue([Decimal('0.00001')], analyzer.get_average_spreads())


class LiquidityAnalyzerTests(unittest.TestCase):

    def test_values_with_no_update_are_correct(self):
        # Arrange
        analyzer = LiquidityAnalyzer()

        # Act
        # Assert
        self.assertEqual(0.0, analyzer.value)
        self.assertFalse(analyzer.initialized)
        self.assertFalse(analyzer.is_liquid)
        self.assertTrue(analyzer.is_not_liquid)

    def test_can_update_with_tick_and_volatility_when_illiquid(self):
        # Arrange
        analyzer = LiquidityAnalyzer()

        # Act
        analyzer.update(Decimal('0.00010'), 0.00010)
        # Assert
        self.assertEqual(1.0, analyzer.value)
        self.assertTrue(analyzer.initialized)
        self.assertFalse(analyzer.is_liquid)
        self.assertTrue(analyzer.is_not_liquid)

    def test_can_update_with_tick_and_volatility_when_liquid(self):
        # Arrange
        analyzer = LiquidityAnalyzer()

        # Act
        analyzer.update(Decimal('0.00002'), 0.00004)
        # Assert
        self.assertEqual(2.0, analyzer.value)
        self.assertTrue(analyzer.initialized)
        self.assertTrue(analyzer.is_liquid)
        self.assertFalse(analyzer.is_not_liquid)

    def test_can_reset(self):
        # Arrange
        analyzer = LiquidityAnalyzer()
        analyzer.update(Decimal('0.00002'), 0.00004)

        # Act
        analyzer.reset()

        # Assert
        self.assertEqual(0.0, analyzer.value)
        self.assertFalse(analyzer.initialized)
        self.assertFalse(analyzer.is_liquid)
        self.assertTrue(analyzer.is_not_liquid)
