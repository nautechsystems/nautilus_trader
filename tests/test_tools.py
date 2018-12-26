#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_tools.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from datetime import datetime, timezone

from inv_indicators.average.ema import ExponentialMovingAverage
from inv_indicators.intrinsic_network import IntrinsicNetwork
from inv_trader.model.objects import Bar, Decimal
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

    def test_can_update_ema_indicator(self):
        # Arrange
        ema = ExponentialMovingAverage(20)
        updater = IndicatorUpdater(ema)
        bar = Bar(
            Decimal('1.00001'),
            Decimal('1.00004'),
            Decimal('1.00002'),
            Decimal('1.00003'),
            1000,
            datetime(1970, 1, 1, 0, 0, 0, 0, timezone.utc))

        # Act
        updater.update_bar(bar)
        result = ema.value

        # Assert
        self.assertEqual(1.00003, result)

    def test_can_update_intrinsic_networks_indicator(self):
        # Arrange
        intrinsic = IntrinsicNetwork(0.2, 0.2)
        updater = IndicatorUpdater(intrinsic, input_method=intrinsic.update_mid)
        bar = Bar(
            Decimal('1.00001'),
            Decimal('1.00004'),
            Decimal('1.00002'),
            Decimal('1.00003'),
            1000,
            datetime(1970, 1, 1, 0, 0, 0, 0, timezone.utc))

        # Act
        updater.update_bar(bar)
        result = intrinsic.state

        # Assert
        self.assertTrue(intrinsic.initialized)
        self.assertEqual(0, result)
