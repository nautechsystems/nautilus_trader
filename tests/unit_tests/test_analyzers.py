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

from inv_trader.analyzers import SpreadAnalyzer
from inv_trader.model.enums import Venue
from inv_trader.model.objects import Symbol, Price, Tick
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = Symbol('AUDUSD', Venue.FXCM)


class SpreadAnalyzerTests(unittest.TestCase):

    def test_can_snapshot_with_no_updates(self):
        # Arrange
        analyzer = SpreadAnalyzer(decimal_precision=5)

        # Act
        analyzer.snapshot_average()

        # Assert
        self.assertFalse(analyzer.initialized)
        self.assertEqual([0.0], analyzer.get_average_spreads())
        self.assertEqual(Decimal(0), analyzer.average)
        self.assertEqual(Decimal, type(analyzer.average))
        self.assertEqual(Decimal, type(analyzer.get_average_spreads()[0]))

    def test_can_update_with_ticks(self):
        # Arrange
        analyzer = SpreadAnalyzer(decimal_precision=5)

        # Act
        analyzer.update(Tick(AUDUSD_FXCM, Price('0.89000'), Price('0.89001'), UNIX_EPOCH))
        analyzer.update(Tick(AUDUSD_FXCM, Price('0.89000'), Price('0.89002'), UNIX_EPOCH))

        # Assert
        self.assertEqual(Decimal('0.00002'), analyzer.average)
        self.assertFalse(analyzer.initialized)

    def test_can_snapshot_average(self):
        # Arrange
        analyzer = SpreadAnalyzer(decimal_precision=5)

        analyzer.update(Tick(AUDUSD_FXCM, Price('0.89000'), Price('0.89001'), UNIX_EPOCH))
        analyzer.update(Tick(AUDUSD_FXCM, Price('0.89000'), Price('0.89002'), UNIX_EPOCH))
        analyzer.update(Tick(AUDUSD_FXCM, Price('0.89001'), Price('0.89004'), UNIX_EPOCH))
        analyzer.update(Tick(AUDUSD_FXCM, Price('0.89001'), Price('0.89001'), UNIX_EPOCH))
        analyzer.update(Tick(AUDUSD_FXCM, Price('0.89002'), Price('0.89001'), UNIX_EPOCH))

        result1 = analyzer.average

        # Act
        analyzer.snapshot_average()
        result2 = analyzer.average

        # Assert
        self.assertEqual(Decimal('0.00001'), result1)
        self.assertEqual(Decimal('0.00001'), result2)
        self.assertTrue(analyzer.initialized)
        self.assertTrue([Decimal('0.00001')], analyzer.get_average_spreads())
