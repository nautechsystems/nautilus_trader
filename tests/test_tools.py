#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_tools.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from inv_indicators.average.ema import ExponentialMovingAverage
from inv_trader.tools import BarBuilder, IndicatorUpdater
from test_kit.data import TestDataProvider


class BarBuilderTests(unittest.TestCase):

    def test_can_build_data_bars(self):
        # arrange
        data = TestDataProvider.get_gbpusd_1min_bid()
        bar_builder = BarBuilder(data, 5, 1)

        # act
        bars = bar_builder.build_data_bars()

        # assert
        self.assertEqual(377280, len(bars))

    def test_can_build_bars(self):
        # arrange
        data = TestDataProvider.get_gbpusd_1min_bid()
        bar_builder = BarBuilder(data, 5, 1)

        # act
        bars = bar_builder.build_bars()

        # assert
        self.assertEqual(377280, len(bars))


class IndicatorUpdaterTests(unittest.TestCase):

    def test_can_update_indicator(self):
        # arrange
        data = TestDataProvider.get_gbpusd_1min_bid()
        bar_builder = BarBuilder(data, 5, 1)
        bars = bar_builder.build_data_bars()
        ema = ExponentialMovingAverage(10)
        updater = IndicatorUpdater(ema)

        # act
        for bar in bars:
            updater.update_bar(bar)

        # assert
        self.assertEqual(377280, ema.count)
        self.assertEqual(1.4640214397333984, ema.value)

    def test_can_build_features(self):
        # arrange
        data = TestDataProvider.get_gbpusd_1min_bid()
        bar_builder = BarBuilder(data, 5, 1)
        bars = bar_builder.build_data_bars()
        ema = ExponentialMovingAverage(10)
        updater = IndicatorUpdater(ema)

        # act
        result = updater.build_features(bars)

        # assert
        self.assertTrue('value' in result)
        self.assertEqual(377280, len(result['value']))
        self.assertEqual(1.4640214397333984, ema.value)
