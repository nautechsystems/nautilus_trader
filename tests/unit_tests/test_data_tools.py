# -------------------------------------------------------------------------------------------------
# <copyright file="test_tools.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from pandas import Timestamp
from datetime import datetime, timezone

from nautilus_trader.model.objects import Price, Bar
from nautilus_trader.data.tools import TickBuilder, BarBuilder, IndicatorUpdater
from inv_indicators.average.ema import ExponentialMovingAverage
from inv_indicators.intrinsic_network import IntrinsicNetwork
from test_kit.data import TestDataProvider
from test_kit.stubs import TestStubs


class TickBuilderTests(unittest.TestCase):

    def setUp(self):
        pass

    def test_tick_data(self):
        # Arrange
        # Act
        ticks = TestDataProvider.usdjpy_test_ticks()

        # Assert
        self.assertEqual(1000, len(ticks))

    def test_build_ticks_all_with_tick_data(self):
        # Arrange
        tick_data = TestDataProvider.usdjpy_test_ticks()
        bid_data = TestDataProvider.usdjpy_1min_bid()[:1000]
        ask_data = TestDataProvider.usdjpy_1min_ask()[:1000]
        self.tick_builder = TickBuilder(symbol=TestStubs.instrument_usdjpy().symbol,
                                        decimal_precision=5,
                                        tick_data=tick_data,
                                        bid_data=bid_data,
                                        ask_data=ask_data)

        # Act
        ticks = self.tick_builder.build_ticks_all()

        # Assert
        self.assertEqual(1000, len(ticks))
        self.assertEqual(Timestamp('2013-01-01 22:02:35.907000', tz='UTC'), ticks[1].timestamp)

    def test_build_ticks_all_with_bar_data(self):
        # Arrange
        bid_data = TestDataProvider.usdjpy_1min_bid()[:1000]
        ask_data = TestDataProvider.usdjpy_1min_ask()[:1000]
        self.tick_builder = TickBuilder(symbol=TestStubs.instrument_usdjpy().symbol,
                                        decimal_precision=5,
                                        tick_data=None,
                                        bid_data=bid_data,
                                        ask_data=ask_data)

        # Act
        ticks = self.tick_builder.build_ticks_all()

        # Assert
        self.assertEqual(1000, len(ticks))
        self.assertEqual(Timestamp('2013-01-01 00:01:00+0000', tz='UTC'), ticks[1].timestamp)


class BarBuilderTests(unittest.TestCase):

    def setUp(self):
        data = TestDataProvider.gbpusd_1min_bid()[:1000]
        self.bar_builder = BarBuilder(5, 1, data)

    def test_build_databars_all(self):
        # Arrange
        # Act
        bars = self.bar_builder.build_databars_all()

        # Assert
        self.assertEqual(1000, len(bars))

    def test_build_databars_range_with_defaults(self):
        # Arrange
        # Act
        bars = self.bar_builder.build_databars_range()

        # Assert
        self.assertEqual(999, len(bars))

    def test_build_databars_range_with_params(self):
        # Arrange
        # Act
        bars = self.bar_builder.build_databars_range(start=500)

        # Assert
        self.assertEqual(499, len(bars))

    def test_build_databars_from_with_defaults(self):
        # Arrange
        # Act
        bars = self.bar_builder.build_databars_from()

        # Assert
        self.assertEqual(1000, len(bars))

    def test_build_databars_from_with_param(self):
        # Arrange
        # Act
        bars = self.bar_builder.build_databars_from(500)

        # Assert
        self.assertEqual(500, len(bars))

    def test_can_build_bars_all(self):
        # Arrange
        # Act
        bars = self.bar_builder.build_bars_all()

        # Assert
        self.assertEqual(1000, len(bars))

    def test_can_build_bars_range_with_defaults(self):
        # Arrange
        # Act
        bars = self.bar_builder.build_bars_range()

        # Assert
        self.assertEqual(999, len(bars))

    def test_can_build_bars_range_with_param(self):
        # Arrange
        # Act
        bars = self.bar_builder.build_bars_range(start=500)

        # Assert
        self.assertEqual(499, len(bars))

    def test_can_build_bars_from_with_defaults(self):
        # Arrange
        # Act
        bars = self.bar_builder.build_bars_from()

        # Assert
        self.assertEqual(1000, len(bars))

    def test_can_build_bars_from_with_param(self):
        # Arrange
        # Act
        bars = self.bar_builder.build_bars_from(index=500)

        # Assert
        self.assertEqual(500, len(bars))


class IndicatorUpdaterTests(unittest.TestCase):

    def test_can_update_indicator_with_bars(self):
        # Arrange
        data = TestDataProvider.gbpusd_1min_bid()[:1000]
        bar_builder = BarBuilder(5, 1, data)
        bars = bar_builder.build_bars_all()
        ema = ExponentialMovingAverage(10)
        updater = IndicatorUpdater(ema)

        # Act
        for bar in bars:
            updater.update_bar(bar)

        # Assert
        self.assertEqual(1000, ema.count)
        self.assertEqual(1.9838850009689002, ema.value)

    def test_can_update_indicator_with_data_bars(self):
        # Arrange
        data = TestDataProvider.gbpusd_1min_bid()[:1000]
        bar_builder = BarBuilder(5, 1, data)
        bars = bar_builder.build_databars_all()
        ema = ExponentialMovingAverage(10)
        updater = IndicatorUpdater(ema)

        # Act
        for bar in bars:
            updater.update_databar(bar)

        # Assert
        self.assertEqual(1000, ema.count)
        self.assertEqual(1.9838850140793318, ema.value)

    def test_can_build_features_from_bars(self):
        # Arrange
        data = TestDataProvider.gbpusd_1min_bid()[:1000]
        bar_builder = BarBuilder(5, 1, data)
        bars = bar_builder.build_bars_all()
        ema = ExponentialMovingAverage(10)
        updater = IndicatorUpdater(ema)

        # Act
        result = updater.build_features(bars)

        # Assert
        self.assertTrue('value' in result)
        self.assertEqual(1000, len(result['value']))
        self.assertEqual(1.9838850009689002, ema.value)

    def test_can_build_features_from_data_bars(self):
        # Arrange
        data = TestDataProvider.gbpusd_1min_bid()[:1000]
        bar_builder = BarBuilder(5, 1, data)
        bars = bar_builder.build_databars_all()
        ema = ExponentialMovingAverage(10)
        updater = IndicatorUpdater(ema)

        # Act
        result = updater.build_features_databars(bars)

        # Assert
        self.assertTrue('value' in result)
        self.assertEqual(1000, len(result['value']))
        self.assertEqual(1.9838850140793318, ema.value)

    def test_can_update_ema_indicator(self):
        # Arrange
        ema = ExponentialMovingAverage(20)
        updater = IndicatorUpdater(ema)

        bar = Bar(
            Price('1.00001'),
            Price('1.00004'),
            Price('1.00002'),
            Price('1.00003'),
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
            Price('1.00001'),
            Price('1.00004'),
            Price('1.00002'),
            Price('1.00003'),
            1000,
            datetime(1970, 1, 1, 0, 0, 0, 0, timezone.utc))

        # Act
        updater.update_bar(bar)
        result = intrinsic.state

        # Assert
        self.assertTrue(intrinsic.initialized)
        self.assertEqual(0, result)
