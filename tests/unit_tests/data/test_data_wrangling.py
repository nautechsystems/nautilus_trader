# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.data.wrangling import QuoteTickDataWrangler
from nautilus_trader.data.wrangling import TradeTickDataWrangler
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import TradeMatchId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.test_kit.providers import TestDataProvider
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestStubs.symbol_audusd_fxcm()


class QuoteTickDataWranglerTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.clock = TestClock()

    def test_tick_data(self):
        # Arrange
        # Act
        ticks = TestDataProvider.usdjpy_ticks()

        # Assert
        self.assertEqual(1000, len(ticks))

    def test_pre_process_with_tick_data(self):
        # Arrange
        tick_data = TestDataProvider.usdjpy_ticks()
        self.tick_builder = QuoteTickDataWrangler(
            instrument=TestInstrumentProvider.default_fx_ccy(TestStubs.symbol_usdjpy_fxcm()),
            data_quotes=tick_data,
            data_bars_bid=None,
            data_bars_ask=None,
        )

        # Act
        self.tick_builder.pre_process(0)
        ticks = self.tick_builder.processed_data

        # Assert
        self.assertEqual(BarAggregation.TICK, self.tick_builder.resolution)
        self.assertEqual(1000, len(ticks))
        self.assertEqual(Timestamp("2013-01-01 22:02:35.907000", tz="UTC"), ticks.iloc[1].name)

    def test_pre_process_with_bar_data(self):
        # Arrange
        bid_data = TestDataProvider.usdjpy_1min_bid()
        ask_data = TestDataProvider.usdjpy_1min_ask()
        self.tick_builder = QuoteTickDataWrangler(
            instrument=TestInstrumentProvider.default_fx_ccy(TestStubs.symbol_usdjpy_fxcm()),
            data_quotes=None,
            data_bars_bid={BarAggregation.MINUTE: bid_data},
            data_bars_ask={BarAggregation.MINUTE: ask_data},
        )

        # Act
        self.tick_builder.pre_process(0)
        tick_data = self.tick_builder.processed_data

        # Assert
        self.assertEqual(BarAggregation.MINUTE, self.tick_builder.resolution)
        self.assertEqual(115044, len(tick_data))
        self.assertEqual(Timestamp("2013-01-31 23:59:59.700000+0000", tz="UTC"), tick_data.iloc[0].name)
        self.assertEqual(Timestamp("2013-01-31 23:59:59.800000+0000", tz="UTC"), tick_data.iloc[1].name)
        self.assertEqual(Timestamp("2013-01-31 23:59:59.900000+0000", tz="UTC"), tick_data.iloc[2].name)
        self.assertEqual(Timestamp("2013-02-01 00:00:00+0000", tz="UTC"), tick_data.iloc[3].name)
        self.assertEqual(0, tick_data.iloc[0]["symbol"])
        self.assertEqual("1", tick_data.iloc[0]["bid_size"])
        self.assertEqual("1", tick_data.iloc[0]["ask_size"])
        self.assertEqual("1", tick_data.iloc[1]["bid_size"])
        self.assertEqual("1", tick_data.iloc[1]["ask_size"])
        self.assertEqual("1", tick_data.iloc[2]["bid_size"])
        self.assertEqual("1", tick_data.iloc[2]["ask_size"])
        self.assertEqual("1", tick_data.iloc[3]["bid_size"])
        self.assertEqual("1", tick_data.iloc[3]["ask_size"])

    def test_build_ticks_with_tick_data(self):
        # Arrange
        tick_data = TestDataProvider.audusd_ticks()
        self.tick_builder = QuoteTickDataWrangler(
            instrument=TestInstrumentProvider.default_fx_ccy(TestStubs.symbol_audusd_fxcm()),
            data_quotes=tick_data,
            data_bars_bid=None,
            data_bars_ask=None,
        )

        # Act
        self.tick_builder.pre_process(0)
        ticks = self.tick_builder.build_ticks()

        # Assert
        self.assertEqual(100000, len(ticks))
        self.assertEqual(Price("0.67067"), ticks[0].bid)
        self.assertEqual(Price("0.67070"), ticks[0].ask)
        self.assertEqual(Quantity("1"), ticks[0].bid_size)
        self.assertEqual(Quantity("1"), ticks[0].ask_size)
        self.assertEqual(Timestamp("2020-01-30 15:28:09.820000+0000", tz="UTC"), ticks[0].timestamp)

    def test_build_ticks_with_bar_data(self):
        # Arrange
        bid_data = TestDataProvider.usdjpy_1min_bid()
        ask_data = TestDataProvider.usdjpy_1min_ask()
        self.tick_builder = QuoteTickDataWrangler(
            instrument=TestInstrumentProvider.default_fx_ccy(TestStubs.symbol_usdjpy_fxcm()),
            data_quotes=None,
            data_bars_bid={BarAggregation.MINUTE: bid_data},
            data_bars_ask={BarAggregation.MINUTE: ask_data},
        )

        # Act
        self.tick_builder.pre_process(0)
        ticks = self.tick_builder.build_ticks()

        # Assert
        self.assertEqual(115044, len(ticks))
        self.assertEqual(Price("91.715"), ticks[0].bid)
        self.assertEqual(Price("91.717"), ticks[0].ask)
        self.assertEqual(Quantity("1"), ticks[0].bid_size)
        self.assertEqual(Quantity("1"), ticks[0].ask_size)
        self.assertEqual(Timestamp("2013-01-31 23:59:59.700000+0000", tz="UTC"), ticks[0].timestamp)


class TradeTickDataWranglerTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.clock = TestClock()

    def test_tick_data(self):
        # Arrange
        # Act
        ticks = TestDataProvider.ethusdt_trades()

        # Assert
        self.assertEqual(69806, len(ticks))

    def test_process(self):
        # Arrange
        tick_data = TestDataProvider.ethusdt_trades()
        self.tick_builder = TradeTickDataWrangler(
            instrument=TestInstrumentProvider.default_fx_ccy(TestStubs.symbol_usdjpy_fxcm()),
            data=tick_data,
        )

        # Act
        self.tick_builder.pre_process(0)
        ticks = self.tick_builder.processed_data

        # Assert
        self.assertEqual(69806, len(ticks))
        self.assertEqual(Timestamp("2020-08-14 10:00:00.223000+0000", tz="UTC"), ticks.iloc[0].name)

    def test_build_ticks(self):
        # Arrange
        tick_data = TestDataProvider.ethusdt_trades()
        self.tick_builder = TradeTickDataWrangler(
            instrument=TestInstrumentProvider.ethusdt_binance(),
            data=tick_data,
        )

        # Act
        self.tick_builder.pre_process(0)
        ticks = self.tick_builder.build_ticks()

        # Assert
        self.assertEqual(69806, len(ticks))
        self.assertEqual(Price("423.760"), ticks[0].price)
        self.assertEqual(Quantity("2.67900"), ticks[0].size)
        self.assertEqual(OrderSide.SELL, ticks[0].side)
        self.assertEqual(TradeMatchId("148568980"), ticks[0].match_id)
        self.assertEqual(Timestamp("2020-08-14 10:00:00.223000+0000", tz="UTC"), ticks[0].timestamp)


class BarDataWranglerTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        data = TestDataProvider.gbpusd_1min_bid()[:1000]
        self.bar_builder = BarDataWrangler(5, 1, data)

    def test_build_bars_all(self):
        # Arrange
        # Act
        bars = self.bar_builder.build_bars_all()

        # Assert
        self.assertEqual(1000, len(bars))

    def test_build_bars_range_with_defaults(self):
        # Arrange
        # Act
        bars = self.bar_builder.build_bars_range()

        # Assert
        self.assertEqual(999, len(bars))

    def test_build_bars_range_with_param(self):
        # Arrange
        # Act
        bars = self.bar_builder.build_bars_range(start=500)

        # Assert
        self.assertEqual(499, len(bars))

    def test_build_bars_from_with_defaults(self):
        # Arrange
        # Act
        bars = self.bar_builder.build_bars_from()

        # Assert
        self.assertEqual(1000, len(bars))

    def test_build_bars_from_with_param(self):
        # Arrange
        # Act
        bars = self.bar_builder.build_bars_from(index=500)

        # Assert
        self.assertEqual(500, len(bars))


class TardisQuoteDataWranglerTests(unittest.TestCase):
    def setUp(self):
        # Fixture Setup
        self.clock = TestClock()

    def test_tick_data(self):
        # Arrange
        # Act
        ticks = TestDataProvider.tardis_quotes()

        # Assert
        self.assertEqual(2069000, len(ticks))

    def test_pre_process_with_tick_data(self):
        # Arrange
        tick_data = TestDataProvider.tardis_quotes()
        self.tick_builder = QuoteTickDataWrangler(
            instrument=TestInstrumentProvider.btcusdt_binance(),
            data_quotes=tick_data,
            data_bars_bid=None,
            data_bars_ask=None,
        )

        # Act
        self.tick_builder.pre_process(0)
        ticks = self.tick_builder.processed_data

        # Assert
        self.assertEqual(BarAggregation.TICK, self.tick_builder.resolution)
        self.assertEqual(2069000, len(ticks))
        self.assertEqual(Timestamp('2020-02-22 00:00:03.522418+0000', tz='UTC'), ticks.iloc[1].name)
        self.assertEqual('0.670000', ticks.bid_size[0])
        self.assertEqual('0.840000', ticks.ask_size[0])
        self.assertEqual('9681.92', ticks.bid[0])
        self.assertEqual('9682.00', ticks.ask[0])
        self.assertEqual(sorted(ticks.columns), sorted(['symbol', 'ask_size', 'ask', 'bid_size', 'bid']))


class TardisTradeDataWranglerTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.clock = TestClock()

    def test_tick_data(self):
        # Arrange
        # Act
        ticks = TestDataProvider.tardis_trades()

        # Assert
        self.assertEqual(137988, len(ticks))

    def test_process(self):
        # Arrange
        tick_data = TestDataProvider.tardis_trades()
        self.tick_builder = TradeTickDataWrangler(
            instrument=TestInstrumentProvider.btcusdt_binance(),
            data=tick_data,
        )

        # Act
        self.tick_builder.pre_process(0)
        ticks = self.tick_builder.processed_data

        # Assert
        self.assertEqual(137988, len(ticks))
        self.assertEqual(Timestamp('2020-02-22 00:00:02.418379+0000', tz='UTC'), ticks.iloc[0].name)

    def test_build_ticks(self):
        # Arrange
        tick_data = TestDataProvider.tardis_trades()
        self.tick_builder = TradeTickDataWrangler(
            instrument=TestInstrumentProvider.btcusdt_binance(),
            data=tick_data,
        )

        # Act
        self.tick_builder.pre_process(0)
        ticks = self.tick_builder.build_ticks()

        # Assert
        self.assertEqual(137988, len(ticks))
        self.assertEqual(Price("9682.00"), ticks[0].price)
        self.assertEqual(Quantity("0.132000"), ticks[0].size)
        self.assertEqual(OrderSide.BUY, ticks[0].side)
        self.assertEqual(TradeMatchId("42377944"), ticks[0].match_id)
        self.assertEqual(Timestamp('2020-02-22 00:00:02.418379+0000', tz='UTC'), ticks[0].timestamp)
