# -------------------------------------------------------------------------------------------------
# <copyright file="test_common_market.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from pandas import Timestamp
from datetime import datetime, timezone

from nautilus_trader.model.enums import BarStructure
from nautilus_trader.model.objects import Price, Tick, Bar
from nautilus_trader.common.market import TickDataWrangler, BarDataWrangler, IndicatorUpdater, BarBuilder
from test_kit.data import TestDataProvider
from test_kit.stubs import TestStubs

from nautilus_indicators.average.ema import ExponentialMovingAverage
from nautilus_indicators.atr import AverageTrueRange

AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()
UNIX_EPOCH = TestStubs.unix_epoch()


class TickDataWranglerTests(unittest.TestCase):

    def setUp(self):
        pass

    def test_tick_data(self):
        # Arrange
        # Act
        ticks = TestDataProvider.usdjpy_test_ticks()

        # Assert
        self.assertEqual(1000, len(ticks))

    def test_build_with_tick_data(self):
        # Arrange
        tick_data = TestDataProvider.usdjpy_test_ticks()
        bid_data = TestDataProvider.usdjpy_1min_bid()
        ask_data = TestDataProvider.usdjpy_1min_ask()
        self.tick_builder = TickDataWrangler(
            instrument=TestStubs.instrument_usdjpy(),
            data_ticks=tick_data,
            data_bars_bid={BarStructure.MINUTE: bid_data},
            data_bars_ask={BarStructure.MINUTE: ask_data})

        # Act
        self.tick_builder.build()
        ticks = self.tick_builder.ticks

        # Assert
        self.assertEqual(BarStructure.TICK, self.tick_builder.resolution)
        self.assertEqual(1000, len(ticks))
        self.assertEqual(Timestamp('2013-01-01 22:02:35.907000', tz='UTC'), ticks[1].timestamp)

    def test_build_ticks_with_bar_data(self):
        # Arrange
        bid_data = TestDataProvider.usdjpy_1min_bid()[:10000]
        ask_data = TestDataProvider.usdjpy_1min_ask()[:10000]
        self.tick_builder = TickDataWrangler(
            instrument=TestStubs.instrument_usdjpy(),
            data_ticks=None,
            data_bars_bid={BarStructure.MINUTE: bid_data},
            data_bars_ask={BarStructure.MINUTE: ask_data})

        # Act
        self.tick_builder.build()
        ticks = self.tick_builder.ticks

        # Assert
        self.assertEqual(BarStructure.MINUTE, self.tick_builder.resolution)
        self.assertEqual(26016, len(ticks))
        self.assertEqual(Timestamp('2013-01-01T21:59:59.900000+00:00', tz='UTC'), ticks[0].timestamp)
        self.assertEqual(Timestamp('2013-01-01T21:59:59.900000+00:00', tz='UTC'), ticks[1].timestamp)
        self.assertEqual(Timestamp('2013-01-01T22:00:00.000000+00:00', tz='UTC'), ticks[2].timestamp)
        self.assertEqual(0, ticks[0].bid_size)
        self.assertEqual(0, ticks[0].ask_size)
        self.assertEqual(0, ticks[1].bid_size)
        self.assertEqual(0, ticks[1].ask_size)
        self.assertEqual(1, ticks[2].bid_size)
        self.assertEqual(2, ticks[2].ask_size)


class BarDataWranglerTests(unittest.TestCase):

    def setUp(self):
        data = TestDataProvider.gbpusd_1min_bid()[:1000]
        self.bar_builder = BarDataWrangler(5, 1, data)

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
        bar_builder = BarDataWrangler(5, 1, data)
        bars = bar_builder.build_bars_all()
        ema = ExponentialMovingAverage(10)
        updater = IndicatorUpdater(ema)

        # Act
        for bar in bars:
            updater.update_bar(bar)

        # Assert
        self.assertEqual(1000, ema.count)
        self.assertEqual(1.9838849991174994, ema.value)

    def test_can_update_indicator_with_data_bars(self):
        # Arrange
        data = TestDataProvider.gbpusd_1min_bid()[:1000]
        bar_builder = BarDataWrangler(5, 1, data)
        bars = bar_builder.build_databars_all()
        ema = ExponentialMovingAverage(10)
        updater = IndicatorUpdater(ema)

        # Act
        for bar in bars:
            updater.update_databar(bar)

        # Assert
        self.assertEqual(1000, ema.count)
        self.assertEqual(1.9838849991174994, ema.value)

    def test_can_build_features_from_bars(self):
        # Arrange
        data = TestDataProvider.gbpusd_1min_bid()[:1000]
        bar_builder = BarDataWrangler(5, 1, data)
        bars = bar_builder.build_bars_all()
        ema = ExponentialMovingAverage(10)
        updater = IndicatorUpdater(ema)

        # Act
        result = updater.build_features_bars(bars)

        # Assert
        self.assertTrue('value' in result)
        self.assertEqual(1000, len(result['value']))
        self.assertEqual(1.9838849991174994, ema.value)

    def test_can_build_features_from_data_bars(self):
        # Arrange
        data = TestDataProvider.gbpusd_1min_bid()[:1000]
        bar_builder = BarDataWrangler(5, 1, data)
        bars = bar_builder.build_databars_all()
        ema = ExponentialMovingAverage(10)
        updater = IndicatorUpdater(ema)

        # Act
        result = updater.build_features_databars(bars)

        # Assert
        self.assertTrue('value' in result)
        self.assertEqual(1000, len(result['value']))
        self.assertEqual(1.9838849991174994, ema.value)

    def test_can_update_ema_indicator(self):
        # Arrange
        ema = ExponentialMovingAverage(20)
        updater = IndicatorUpdater(ema)

        bar = Bar(
            Price(1.00001, 5),
            Price(1.00004, 5),
            Price(1.00002, 5),
            Price(1.00003, 5),
            1000,
            datetime(1970, 1, 1, 0, 0, 0, 0, timezone.utc))

        # Act
        updater.update_bar(bar)
        result = ema.value

        # Assert
        self.assertEqual(1.0000300407409668, result)

    def test_can_update_atr_indicator(self):
        # Arrange
        atr = AverageTrueRange(10)
        updater = IndicatorUpdater(atr, input_method=atr.update)

        bar = Bar(
            Price(1.00001, 5),
            Price(1.00004, 5),
            Price(1.00002, 5),
            Price(1.00003, 5),
            1000,
            datetime(1970, 1, 1, 0, 0, 0, 0, timezone.utc))

        # Act
        updater.update_bar(bar)
        result = atr.value

        # Assert
        self.assertEqual(2.002716064453125e-05, result)


class BarBuilderTests(unittest.TestCase):

    def test_build_with_no_updates(self):
        # Arrange
        bar_spec = TestStubs.bar_spec_1min_mid()
        builder = BarBuilder(bar_spec, use_previous_close=False)

        builder.build()

        # Act
        bar = builder.build()  # Also resets builder

        # Assert
        self.assertEqual(None, bar.open)
        self.assertEqual(None, bar.high)
        self.assertEqual(None, bar.low)
        self.assertEqual(None, bar.close)
        self.assertEqual(0, bar.volume)
        self.assertEqual(0, builder.count)
        self.assertEqual(None, builder.last_update)

    def test_update(self):
        # Arrange
        bar_spec = TestStubs.bar_spec_1min_mid()
        builder = BarBuilder(bar_spec, use_previous_close=True)

        tick1 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price(1.00001, 5),
            ask=Price(1.00004, 5),
            timestamp=UNIX_EPOCH)

        tick2 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price(1.00002, 5),
            ask=Price(1.00005, 5),
            timestamp=UNIX_EPOCH)

        tick3 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price(1.00000, 5),
            ask=Price(1.00003, 5),
            timestamp=UNIX_EPOCH)

        # Act
        builder.update(tick1)
        builder.update(tick2)
        builder.update(tick3)

        # Assert
        self.assertEqual(bar_spec, builder.bar_spec)
        self.assertEqual(3, builder.count)
        self.assertEqual(UNIX_EPOCH, builder.last_update)

    def test_build_bid(self):
        # Arrange
        bar_spec = TestStubs.bar_spec_1min_bid()
        builder = BarBuilder(bar_spec, use_previous_close=True)

        tick1 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price(1.00001, 5),
            ask=Price(1.00004, 5),
            timestamp=UNIX_EPOCH)

        tick2 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price(1.00002, 5),
            ask=Price(1.00005, 5),
            timestamp=UNIX_EPOCH)

        tick3 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price(1.00000, 5),
            ask=Price(1.00003, 5),
            timestamp=UNIX_EPOCH)

        builder.update(tick1)
        builder.update(tick2)
        builder.update(tick3)

        # Act
        bar = builder.build()  # Also resets builder

        # Assert
        self.assertEqual(Price(1.00001, 5), bar.open)
        self.assertEqual(Price(1.00002, 5), bar.high)
        self.assertEqual(Price(1.00000, 5), bar.low)
        self.assertEqual(Price(1.00000, 5), bar.close)
        self.assertEqual(3, bar.volume)
        self.assertEqual(UNIX_EPOCH, bar.timestamp)
        self.assertEqual(UNIX_EPOCH, builder.last_update)
        self.assertEqual(0, builder.count)

    def test_build_mid(self):
        # Arrange
        bar_spec = TestStubs.bar_spec_1min_mid()
        builder = BarBuilder(bar_spec, use_previous_close=True)

        tick1 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price(1.00001, 5),
            ask=Price(1.00004, 5),
            timestamp=UNIX_EPOCH)

        tick2 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price(1.00002, 5),
            ask=Price(1.00005, 5),
            timestamp=UNIX_EPOCH)

        tick3 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price(1.00000, 5),
            ask=Price(1.00003, 5),
            timestamp=UNIX_EPOCH)

        builder.update(tick1)
        builder.update(tick2)
        builder.update(tick3)

        # Act
        bar = builder.build()  # Also resets builder

        # Assert
        self.assertEqual(Price(1.000025, 6), bar.open)
        self.assertEqual(Price(1.000035, 6), bar.high)
        self.assertEqual(Price(1.000015, 6), bar.low)
        self.assertEqual(Price(1.000015, 6), bar.close)
        self.assertEqual(3, bar.volume)
        self.assertEqual(UNIX_EPOCH, bar.timestamp)
        self.assertEqual(UNIX_EPOCH, builder.last_update)
        self.assertEqual(0, builder.count)

    def test_build_with_previous_close(self):
        # Arrange
        bar_spec = TestStubs.bar_spec_1min_mid()
        builder = BarBuilder(bar_spec, use_previous_close=True)

        tick1 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price(1.00001, 5),
            ask=Price(1.00004, 5),
            timestamp=UNIX_EPOCH)

        tick2 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price(1.00002, 5),
            ask=Price(1.00005, 5),
            timestamp=UNIX_EPOCH)

        tick3 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price(1.00000, 5),
            ask=Price(1.00003, 5),
            timestamp=UNIX_EPOCH)

        builder.update(tick1)
        builder.update(tick2)
        builder.update(tick3)
        builder.build()

        # Act
        bar = builder.build()  # Also resets builder

        # Assert
        self.assertEqual(Price(1.000015, 6), bar.open)
        self.assertEqual(Price(1.000015, 6), bar.high)
        self.assertEqual(Price(1.000015, 6), bar.low)
        self.assertEqual(Price(1.000015, 6), bar.close)
        self.assertEqual(0, bar.volume)
        self.assertEqual(UNIX_EPOCH, bar.timestamp)
        self.assertEqual(UNIX_EPOCH, builder.last_update)
        self.assertEqual(0, builder.count)
