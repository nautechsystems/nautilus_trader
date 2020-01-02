# -------------------------------------------------------------------------------------------------
# <copyright file="test_common_data.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from datetime import datetime, timedelta, timezone

from nautilus_trader.common.logger import TestLogger
from nautilus_trader.common.data import TickBarAggregator, TimeBarTimedAggregator
from nautilus_trader.model.enums import BarStructure, QuoteType
from nautilus_trader.model.objects import Price, Tick, BarSpecification, BarType

from test_kit.stubs import TestStubs

AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()
UNIX_EPOCH = TestStubs.unix_epoch()


class TickBarAggregatorTests(unittest.TestCase):

    def test_update_sends_bar_to_handler(self):
        # Arrange
        bar_store = []
        handler = bar_store.append
        symbol = TestStubs.symbol_audusd_fxcm()
        bar_spec = BarSpecification(3, BarStructure.TICK, QuoteType.MID)
        bar_type = BarType(symbol, bar_spec)
        aggregator = TickBarAggregator(bar_type, handler, TestLogger())

        tick1 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price('1.00001'),
            ask=Price('1.00004'),
            timestamp=UNIX_EPOCH)

        tick2 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price('1.00002'),
            ask=Price('1.00005'),
            timestamp=UNIX_EPOCH)

        tick3 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price('1.00000'),
            ask=Price('1.00003'),
            timestamp=UNIX_EPOCH)

        # Act
        aggregator.update(tick1)
        aggregator.update(tick2)
        aggregator.update(tick3)

        # Assert
        self.assertEqual(1, len(bar_store))
        self.assertEqual(Price('1.000025'), bar_store[0].open)
        self.assertEqual(Price('1.000035'), bar_store[0].high)
        self.assertEqual(Price('1.000015'), bar_store[0].low)
        self.assertEqual(Price('1.000015'), bar_store[0].close)
        self.assertEqual(3, bar_store[0].volume)


class TimeBarAggregatorTests(unittest.TestCase):

    def test_update_sends_bar_to_handler(self):
        # Arrange
        bar_store = []
        handler = bar_store.append
        symbol = TestStubs.symbol_audusd_fxcm()
        bar_spec = BarSpecification(1, BarStructure.MINUTE, QuoteType.MID)
        bar_type = BarType(symbol, bar_spec)
        aggregator = TimeBarTimedAggregator(bar_type, handler, TestLogger())

        stop_time = UNIX_EPOCH + timedelta(minutes=2)

        tick1 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price('1.00001'),
            ask=Price('1.00004'),
            timestamp=UNIX_EPOCH)

        tick2 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price('1.00002'),
            ask=Price('1.00005'),
            timestamp=UNIX_EPOCH)

        tick3 = Tick(
            symbol=AUDUSD_FXCM,
            bid=Price('1.00000'),
            ask=Price('1.00003'),
            timestamp=stop_time)

        # Act
        aggregator.update(tick1)
        aggregator.update(tick2)
        aggregator.update(tick3)

        # Assert
        self.assertEqual(1, len(bar_store))
        self.assertEqual(Price('1.000025'), bar_store[0].open)
        self.assertEqual(Price('1.000035'), bar_store[0].high)
        self.assertEqual(Price('1.000015'), bar_store[0].low)
        self.assertEqual(Price('1.000015'), bar_store[0].close)
        self.assertEqual(3, bar_store[0].volume)
        self.assertEqual(datetime(1970, 1, 1, 0, 2, tzinfo=timezone.utc), bar_store[0].timestamp)
