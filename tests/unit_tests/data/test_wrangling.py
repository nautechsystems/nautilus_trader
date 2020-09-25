# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import unittest

from pandas import Timestamp

from nautilus_trader.common.clock import TestClock
from nautilus_trader.data.wrangling import BarDataWrangler
from nautilus_trader.data.wrangling import TickDataWrangler
from nautilus_trader.model.enums import BarAggregation
from tests.test_kit.data import TestDataProvider
from tests.test_kit.stubs import TestStubs

AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()


class TickDataWranglerTests(unittest.TestCase):

    def setUp(self):
        self.clock = TestClock()

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
            data_bars_bid={BarAggregation.MINUTE: bid_data},
            data_bars_ask={BarAggregation.MINUTE: ask_data})

        # Act
        self.tick_builder.pre_process(0)
        ticks = self.tick_builder.tick_data

        # Assert
        self.assertEqual(BarAggregation.TICK, self.tick_builder.resolution)
        self.assertEqual(1000, len(ticks))
        self.assertEqual(Timestamp("2013-01-01 22:02:35.907000", tz="UTC"), ticks.iloc[1].name)

    def test_build_ticks_with_bar_data(self):
        # Arrange
        bid_data = TestDataProvider.usdjpy_1min_bid()
        ask_data = TestDataProvider.usdjpy_1min_ask()
        self.tick_builder = TickDataWrangler(
            instrument=TestStubs.instrument_usdjpy(),
            data_ticks=None,
            data_bars_bid={BarAggregation.MINUTE: bid_data},
            data_bars_ask={BarAggregation.MINUTE: ask_data})

        # Act
        self.tick_builder.pre_process(0)
        tick_data = self.tick_builder.tick_data

        # Assert
        self.assertEqual(BarAggregation.MINUTE, self.tick_builder.resolution)
        self.assertEqual(1491252, len(tick_data))
        self.assertEqual(Timestamp("2013-01-01T21:59:59.900000+00:00", tz="UTC"), tick_data.iloc[0].name)
        self.assertEqual(Timestamp("2013-01-01T21:59:59.900000+00:00", tz="UTC"), tick_data.iloc[1].name)
        self.assertEqual(Timestamp("2013-01-01T21:59:59.900000+00:00", tz="UTC"), tick_data.iloc[2].name)
        self.assertEqual(Timestamp("2013-01-01T22:00:00.000000+00:00", tz="UTC"), tick_data.iloc[3].name)
        self.assertEqual(0, tick_data.iloc[0]["symbol"])
        self.assertEqual(0, tick_data.iloc[0]["bid_size"])
        self.assertEqual(0, tick_data.iloc[0]["ask_size"])
        self.assertEqual(0, tick_data.iloc[1]["bid_size"])
        self.assertEqual(0, tick_data.iloc[1]["ask_size"])
        self.assertEqual(0, tick_data.iloc[2]["bid_size"])
        self.assertEqual(0, tick_data.iloc[2]["ask_size"])
        self.assertEqual(1.5, tick_data.iloc[3]["bid_size"])
        self.assertEqual(2.25, tick_data.iloc[3]["ask_size"])


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
