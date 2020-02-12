# -------------------------------------------------------------------------------------------------
# <copyright file="test_common_market.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
from pandas import Timestamp
from datetime import datetime, timedelta, timezone

from nautilus_indicators.average.ema import ExponentialMovingAverage
from nautilus_indicators.atr import AverageTrueRange
from nautilus_trader.model.enums import PriceType, BarStructure
from nautilus_trader.model.objects import Price, Volume, Tick, Bar, BarSpecification, BarType
from nautilus_trader.common.market import TickDataWrangler, BarDataWrangler, IndicatorUpdater
from nautilus_trader.common.market import BarBuilder, TickBarAggregator, TimeBarAggregator
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logger import TestLogger
from test_kit.data import TestDataProvider
from test_kit.stubs import TestStubs, UNIX_EPOCH
from test_kit.mocks import ObjectStorer

AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()


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
        self.tick_builder.build(0)
        ticks = self.tick_builder.tick_data

        # Assert
        self.assertEqual(BarStructure.TICK, self.tick_builder.resolution)
        self.assertEqual(1000, len(ticks))
        self.assertEqual(Timestamp('2013-01-01 22:02:35.907000', tz='UTC'), ticks.iloc[1].name)

    def test_build_ticks_with_bar_data(self):
        # Arrange
        bid_data = TestDataProvider.usdjpy_1min_bid()
        ask_data = TestDataProvider.usdjpy_1min_ask()
        self.tick_builder = TickDataWrangler(
            instrument=TestStubs.instrument_usdjpy(),
            data_ticks=None,
            data_bars_bid={BarStructure.MINUTE: bid_data},
            data_bars_ask={BarStructure.MINUTE: ask_data})

        # Act
        self.tick_builder.build(0)
        tick_data = self.tick_builder.tick_data

        # Assert
        self.assertEqual(BarStructure.MINUTE, self.tick_builder.resolution)
        self.assertEqual(1118439, len(tick_data))
        self.assertEqual(Timestamp('2013-01-01T21:59:59.900000+00:00', tz='UTC'), tick_data.iloc[0].name)
        self.assertEqual(Timestamp('2013-01-01T21:59:59.900000+00:00', tz='UTC'), tick_data.iloc[1].name)
        self.assertEqual(Timestamp('2013-01-01T22:00:00.000000+00:00', tz='UTC'), tick_data.iloc[2].name)
        self.assertEqual(0, tick_data.iloc[0]['symbol'])
        self.assertEqual(0, tick_data.iloc[0]['bid_size'])
        self.assertEqual(0, tick_data.iloc[0]['ask_size'])
        self.assertEqual(0, tick_data.iloc[1]['bid_size'])
        self.assertEqual(0, tick_data.iloc[1]['ask_size'])
        self.assertEqual(1.5, tick_data.iloc[2]['bid_size'])
        self.assertEqual(2.25, tick_data.iloc[2]['ask_size'])


class BarDataWranglerTests(unittest.TestCase):

    def setUp(self):
        data = TestDataProvider.gbpusd_1min_bid()[:1000]
        self.bar_builder = BarDataWrangler(5, 1, data)

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
        self.assertEqual(1.9838850009689002, ema.value)

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
        self.assertEqual(1.9838850009689002, ema.value)

    def test_can_update_ema_indicator(self):
        # Arrange
        ema = ExponentialMovingAverage(20)
        updater = IndicatorUpdater(ema)

        bar = Bar(
            Price(1.00001, 5),
            Price(1.00004, 5),
            Price(1.00002, 5),
            Price(1.00003, 5),
            Volume(1000),
            datetime(1970, 1, 1, 0, 0, 0, 0, timezone.utc))

        # Act
        updater.update_bar(bar)
        result = ema.value

        # Assert
        self.assertEqual(1.00003, result)

    def test_can_update_atr_indicator(self):
        # Arrange
        atr = AverageTrueRange(10)
        updater = IndicatorUpdater(atr, input_method=atr.update)

        bar = Bar(
            Price(1.00001, 5),
            Price(1.00004, 5),
            Price(1.00002, 5),
            Price(1.00003, 5),
            Volume(1000),
            datetime(1970, 1, 1, 0, 0, 0, 0, timezone.utc))

        # Act
        updater.update_bar(bar)
        result = atr.value

        # Assert
        self.assertEqual(2.0000000000131024e-05, result)


class BarBuilderTests(unittest.TestCase):

    def test_build_with_no_updates_raises_exception(self):
        # Arrange
        bar_spec = TestStubs.bar_spec_1min_mid()
        builder = BarBuilder(bar_spec, use_previous_close=False)

        # Act
        # Assert
        self.assertRaises(TypeError, builder.build)

    def test_update(self):
        # Arrange
        bar_spec = TestStubs.bar_spec_1min_mid()
        builder = BarBuilder(bar_spec, use_previous_close=True)

        tick1 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price(1.00001, 5),
            ask=Price(1.00004, 5),
            bid_size=Volume(1),
            ask_size=Volume(1),
            timestamp=UNIX_EPOCH)

        tick2 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price(1.00002, 5),
            ask=Price(1.00005, 5),
            bid_size=Volume(1),
            ask_size=Volume(1),
            timestamp=UNIX_EPOCH)

        tick3 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price(1.00000, 5),
            ask=Price(1.00003, 5),
            bid_size=Volume(1),
            ask_size=Volume(1),
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
            bid_size=Volume(1),
            ask_size=Volume(1),
            timestamp=UNIX_EPOCH)

        tick2 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price(1.00002, 5),
            ask=Price(1.00005, 5),
            bid_size=Volume(1),
            ask_size=Volume(1),
            timestamp=UNIX_EPOCH)

        tick3 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price(1.00000, 5),
            ask=Price(1.00003, 5),
            bid_size=Volume(1),
            ask_size=Volume(1),
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
        self.assertEqual(3, bar.volume.as_int())
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
            bid_size=Volume(1),
            ask_size=Volume(1),
            timestamp=UNIX_EPOCH)

        tick2 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price(1.00002, 5),
            ask=Price(1.00005, 5),
            bid_size=Volume(1),
            ask_size=Volume(1),
            timestamp=UNIX_EPOCH)

        tick3 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price(1.00000, 5),
            ask=Price(1.00003, 5),
            bid_size=Volume(1),
            ask_size=Volume(1),
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
        self.assertEqual(3, bar.volume.as_int())
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
            bid_size=Volume(1),
            ask_size=Volume(1),
            timestamp=UNIX_EPOCH)

        tick2 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price(1.00002, 5),
            ask=Price(1.00005, 5),
            bid_size=Volume(1),
            ask_size=Volume(1),
            timestamp=UNIX_EPOCH)

        tick3 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price(1.00000, 5),
            ask=Price(1.00003, 5),
            bid_size=Volume(1),
            ask_size=Volume(1),
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
        self.assertEqual(0, bar.volume.as_int())
        self.assertEqual(UNIX_EPOCH, bar.timestamp)
        self.assertEqual(UNIX_EPOCH, builder.last_update)
        self.assertEqual(0, builder.count)


class TickBarAggregatorTests(unittest.TestCase):

    def test_update_sends_bar_to_handler(self):
        # Arrange
        bar_store = ObjectStorer()
        handler = bar_store.store_2
        symbol = TestStubs.symbol_audusd_fxcm()
        bar_spec = BarSpecification(3, BarStructure.TICK, PriceType.MID)
        bar_type = BarType(symbol, bar_spec)
        aggregator = TickBarAggregator(bar_type, handler, TestLogger())

        tick1 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price(1.00001, 5),
            ask=Price(1.00004, 5),
            bid_size=Volume(1),
            ask_size=Volume(1),
            timestamp=UNIX_EPOCH)

        tick2 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price(1.00002, 5),
            ask=Price(1.00005, 5),
            bid_size=Volume(1),
            ask_size=Volume(1),
            timestamp=UNIX_EPOCH)

        tick3 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price(1.00000, 5),
            ask=Price(1.00003, 5),
            bid_size=Volume(1),
            ask_size=Volume(1),
            timestamp=UNIX_EPOCH)

        # Act
        aggregator.update(tick1)
        aggregator.update(tick2)
        aggregator.update(tick3)

        # Assert
        self.assertEqual(1, len(bar_store.get_store()))
        self.assertEqual(Price(1.000025, 6), bar_store.get_store()[0][1].open)
        self.assertEqual(Price(1.000035, 6), bar_store.get_store()[0][1].high)
        self.assertEqual(Price(1.000015, 6), bar_store.get_store()[0][1].low)
        self.assertEqual(Price(1.000015, 6), bar_store.get_store()[0][1].close)
        self.assertEqual(3, bar_store.get_store()[0][1].volume)


class TimeBarAggregatorTests(unittest.TestCase):

    def test_update_timed_with_test_clock_sends_bars_to_handler(self):
        # Arrange
        bar_store = ObjectStorer()
        handler = bar_store.store_2
        symbol = TestStubs.symbol_audusd_fxcm()
        bar_spec = BarSpecification(1, BarStructure.MINUTE, PriceType.MID)
        bar_type = BarType(symbol, bar_spec)
        aggregator = TimeBarAggregator(bar_type, handler, TestClock(), TestLogger())

        stop_time = UNIX_EPOCH + timedelta(minutes=2)

        tick1 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price(1.00001, 5),
            ask=Price(1.00004, 5),
            bid_size=Volume(1),
            ask_size=Volume(1),
            timestamp=UNIX_EPOCH)

        tick2 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price(1.00002, 5),
            ask=Price(1.00005, 5),
            bid_size=Volume(1),
            ask_size=Volume(1),
            timestamp=UNIX_EPOCH)

        tick3 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price(1.00000, 5),
            ask=Price(1.00003, 5),
            bid_size=Volume(1),
            ask_size=Volume(1),
            timestamp=stop_time)

        # Act
        aggregator.update(tick1)
        aggregator.update(tick2)
        aggregator.update(tick3)

        # Assert
        self.assertEqual(2, len(bar_store.get_store()))
        self.assertEqual(Price(1.000025, 6), bar_store.get_store()[0][1].open)
        self.assertEqual(Price(1.000035, 6), bar_store.get_store()[0][1].high)
        self.assertEqual(Price(1.000015, 6), bar_store.get_store()[0][1].low)
        self.assertEqual(Price(1.000015, 6), bar_store.get_store()[0][1].close)
        self.assertEqual(3, bar_store.get_store()[0][1].volume)
        self.assertEqual(0, bar_store.get_store()[1][1].volume)
        self.assertEqual(datetime(1970, 1, 1, 0, 1, tzinfo=timezone.utc), bar_store.get_store()[0][1].timestamp)
        self.assertEqual(datetime(1970, 1, 1, 0, 2, tzinfo=timezone.utc), bar_store.get_store()[1][1].timestamp)
