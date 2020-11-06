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

from datetime import datetime
from datetime import timedelta
import unittest

import pytz

from nautilus_trader.backtest.loaders import InstrumentLoader
from nautilus_trader.backtest.logging import TestLogger
from nautilus_trader.common.clock import TestClock
from nautilus_trader.data.aggregation import BarBuilder
from nautilus_trader.data.aggregation import BulkTickBarBuilder
from nautilus_trader.data.aggregation import TickBarAggregator
from nautilus_trader.data.aggregation import TimeBarAggregator
from nautilus_trader.data.wrangling import TickDataWrangler
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.bar import BarType
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.tick import QuoteTick
from tests.test_kit.data import TestDataProvider
from tests.test_kit.mocks import ObjectStorer
from tests.test_kit.stubs import TestStubs
from tests.test_kit.stubs import UNIX_EPOCH


AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()


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

        tick1 = QuoteTick(
            symbol=AUDUSD_FXCM,
            bid=Price("1.00001"),
            ask=Price("1.00004"),
            bid_size=Quantity(1),
            ask_size=Quantity(1),
            timestamp=UNIX_EPOCH,
        )

        tick2 = QuoteTick(
            symbol=AUDUSD_FXCM,
            bid=Price("1.00002"),
            ask=Price("1.00005"),
            bid_size=Quantity(1),
            ask_size=Quantity(1),
            timestamp=UNIX_EPOCH,
        )

        tick3 = QuoteTick(
            symbol=AUDUSD_FXCM,
            bid=Price("1.00000"),
            ask=Price("1.00003"),
            bid_size=Quantity(1),
            ask_size=Quantity(1),
            timestamp=UNIX_EPOCH,
        )

        # Act
        builder.handle_quote_tick(tick1)
        builder.handle_quote_tick(tick2)
        builder.handle_quote_tick(tick3)

        # Assert
        self.assertEqual(bar_spec, builder.bar_spec)
        self.assertEqual(3, builder.count)
        self.assertEqual(UNIX_EPOCH, builder.last_update)

    def test_build_bid(self):
        # Arrange
        bar_spec = TestStubs.bar_spec_1min_bid()
        builder = BarBuilder(bar_spec, use_previous_close=True)

        tick1 = QuoteTick(
            symbol=AUDUSD_FXCM,
            bid=Price("1.00001"),
            ask=Price("1.00004"),
            bid_size=Quantity(1),
            ask_size=Quantity(1),
            timestamp=UNIX_EPOCH,
        )

        tick2 = QuoteTick(
            symbol=AUDUSD_FXCM,
            bid=Price("1.00002"),
            ask=Price("1.00005"),
            bid_size=Quantity(1),
            ask_size=Quantity(1),
            timestamp=UNIX_EPOCH,
        )

        tick3 = QuoteTick(
            symbol=AUDUSD_FXCM,
            bid=Price("1.00000"),
            ask=Price("1.00003"),
            bid_size=Quantity(1),
            ask_size=Quantity(1),
            timestamp=UNIX_EPOCH,
        )

        builder.handle_quote_tick(tick1)
        builder.handle_quote_tick(tick2)
        builder.handle_quote_tick(tick3)

        # Act
        bar = builder.build()  # Also resets builder

        # Assert
        self.assertEqual(Price("1.00001"), bar.open)
        self.assertEqual(Price("1.00002"), bar.high)
        self.assertEqual(Price("1.00000"), bar.low)
        self.assertEqual(Price("1.00000"), bar.close)
        self.assertEqual(Quantity(3), bar.volume)
        self.assertEqual(UNIX_EPOCH, bar.timestamp)
        self.assertEqual(UNIX_EPOCH, builder.last_update)
        self.assertEqual(0, builder.count)

    def test_build_mid(self):
        # Arrange
        bar_spec = TestStubs.bar_spec_1min_mid()
        builder = BarBuilder(bar_spec, use_previous_close=True)

        tick1 = QuoteTick(
            symbol=AUDUSD_FXCM,
            bid=Price("1.00001"),
            ask=Price("1.00004"),
            bid_size=Quantity(1),
            ask_size=Quantity(1),
            timestamp=UNIX_EPOCH,
        )

        tick2 = QuoteTick(
            symbol=AUDUSD_FXCM,
            bid=Price("1.00002"),
            ask=Price("1.00005"),
            bid_size=Quantity(1),
            ask_size=Quantity(1),
            timestamp=UNIX_EPOCH,
        )

        tick3 = QuoteTick(
            symbol=AUDUSD_FXCM,
            bid=Price("1.00000"),
            ask=Price("1.00003"),
            bid_size=Quantity(1),
            ask_size=Quantity(1),
            timestamp=UNIX_EPOCH,
        )

        builder.handle_quote_tick(tick1)
        builder.handle_quote_tick(tick2)
        builder.handle_quote_tick(tick3)

        # Act
        bar = builder.build()  # Also resets builder

        # Assert
        self.assertEqual(Price("1.000025"), bar.open)
        self.assertEqual(Price("1.000035"), bar.high)
        self.assertEqual(Price("1.000015"), bar.low)
        self.assertEqual(Price("1.000015"), bar.close)
        self.assertEqual(Quantity(6), bar.volume)
        self.assertEqual(UNIX_EPOCH, bar.timestamp)
        self.assertEqual(UNIX_EPOCH, builder.last_update)
        self.assertEqual(0, builder.count)

    def test_build_with_previous_close(self):
        # Arrange
        bar_spec = TestStubs.bar_spec_1min_mid()
        builder = BarBuilder(bar_spec, use_previous_close=True)

        tick1 = QuoteTick(
            symbol=AUDUSD_FXCM,
            bid=Price("1.00001"),
            ask=Price("1.00004"),
            bid_size=Quantity(1),
            ask_size=Quantity(1),
            timestamp=UNIX_EPOCH,
        )

        tick2 = QuoteTick(
            symbol=AUDUSD_FXCM,
            bid=Price("1.00002"),
            ask=Price("1.00005"),
            bid_size=Quantity(1),
            ask_size=Quantity(1),
            timestamp=UNIX_EPOCH,
        )

        tick3 = QuoteTick(
            symbol=AUDUSD_FXCM,
            bid=Price("1.00000"),
            ask=Price("1.00003"),
            bid_size=Quantity(1),
            ask_size=Quantity(1),
            timestamp=UNIX_EPOCH,
        )

        builder.handle_quote_tick(tick1)
        builder.handle_quote_tick(tick2)
        builder.handle_quote_tick(tick3)
        builder.build()

        # Act
        bar = builder.build()  # Also resets builder

        # Assert
        self.assertEqual(Price("1.000015"), bar.open)
        self.assertEqual(Price("1.000015"), bar.high)
        self.assertEqual(Price("1.000015"), bar.low)
        self.assertEqual(Price("1.000015"), bar.close)
        self.assertEqual(Quantity(), bar.volume)
        self.assertEqual(UNIX_EPOCH, bar.timestamp)
        self.assertEqual(UNIX_EPOCH, builder.last_update)
        self.assertEqual(0, builder.count)


class TickBarAggregatorTests(unittest.TestCase):

    def test_update_sends_bar_to_handler(self):
        # Arrange
        bar_store = ObjectStorer()
        handler = bar_store.store_2
        symbol = TestStubs.symbol_audusd_fxcm()
        bar_spec = BarSpecification(3, BarAggregation.TICK, PriceType.MID)
        bar_type = BarType(symbol, bar_spec)
        aggregator = TickBarAggregator(bar_type, handler, TestLogger(TestClock()))

        tick1 = QuoteTick(
            symbol=AUDUSD_FXCM,
            bid=Price("1.00001"),
            ask=Price("1.00004"),
            bid_size=Quantity(1),
            ask_size=Quantity(1),
            timestamp=UNIX_EPOCH,
        )

        tick2 = QuoteTick(
            symbol=AUDUSD_FXCM,
            bid=Price("1.00002"),
            ask=Price("1.00005"),
            bid_size=Quantity(1),
            ask_size=Quantity(1),
            timestamp=UNIX_EPOCH,
        )

        tick3 = QuoteTick(
            symbol=AUDUSD_FXCM,
            bid=Price("1.00000"),
            ask=Price("1.00003"),
            bid_size=Quantity(1),
            ask_size=Quantity(1),
            timestamp=UNIX_EPOCH,
        )

        # Act
        aggregator.handle_quote_tick(tick1)
        aggregator.handle_quote_tick(tick2)
        aggregator.handle_quote_tick(tick3)

        # Assert
        self.assertEqual(1, len(bar_store.get_store()))
        self.assertEqual(Price("1.000025"), bar_store.get_store()[0][1].open)
        self.assertEqual(Price("1.000035"), bar_store.get_store()[0][1].high)
        self.assertEqual(Price("1.000015"), bar_store.get_store()[0][1].low)
        self.assertEqual(Price('1.000015'), bar_store.get_store()[0][1].close)
        self.assertEqual(Quantity(6), bar_store.get_store()[0][1].volume)


class TimeBarAggregatorTests(unittest.TestCase):

    def test_update_timed_with_test_clock_sends_single_bar_to_handler(self):
        # Arrange
        clock = TestClock()
        bar_store = ObjectStorer()
        handler = bar_store.store_2
        symbol = TestStubs.symbol_audusd_fxcm()
        bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.MID)
        bar_type = BarType(symbol, bar_spec)
        aggregator = TimeBarAggregator(bar_type, handler, True, TestClock(), TestLogger(clock))

        stop_time = UNIX_EPOCH + timedelta(minutes=2)

        tick1 = QuoteTick(
            symbol=AUDUSD_FXCM,
            bid=Price("1.00001"),
            ask=Price("1.00004"),
            bid_size=Quantity(1),
            ask_size=Quantity(1),
            timestamp=UNIX_EPOCH,
        )

        tick2 = QuoteTick(
            symbol=AUDUSD_FXCM,
            bid=Price("1.00002"),
            ask=Price("1.00005"),
            bid_size=Quantity(1),
            ask_size=Quantity(1),
            timestamp=UNIX_EPOCH,
        )

        tick3 = QuoteTick(
            symbol=AUDUSD_FXCM,
            bid=Price("1.00000"),
            ask=Price("1.00003"),
            bid_size=Quantity(1),
            ask_size=Quantity(1),
            timestamp=stop_time,
        )

        # Act
        aggregator.handle_quote_tick(tick1)
        aggregator.handle_quote_tick(tick2)
        aggregator.handle_quote_tick(tick3)

        # Assert
        self.assertEqual(1, len(bar_store.get_store()))
        self.assertEqual(Price("1.000025"), bar_store.get_store()[0][1].open)
        self.assertEqual(Price("1.000035"), bar_store.get_store()[0][1].high)
        self.assertEqual(Price("1.000025"), bar_store.get_store()[0][1].low)
        self.assertEqual(Price("1.000035"), bar_store.get_store()[0][1].close)
        self.assertEqual(Quantity(4), bar_store.get_store()[0][1].volume)
        self.assertEqual(datetime(1970, 1, 1, 0, 1, tzinfo=pytz.utc), bar_store.get_store()[0][1].timestamp)


class BulkTickBarBuilderTests(unittest.TestCase):

    def test_given_list_of_ticks_aggregates_tick_bars(self):
        # Arrange
        tick_data = TestDataProvider.usdjpy_test_ticks()
        bid_data = TestDataProvider.usdjpy_1min_bid()
        ask_data = TestDataProvider.usdjpy_1min_ask()
        self.wrangler = TickDataWrangler(
            instrument=InstrumentLoader.default_fx_ccy(TestStubs.symbol_usdjpy_fxcm()),
            data_ticks=tick_data,
            data_bars_bid={BarAggregation.MINUTE: bid_data},
            data_bars_ask={BarAggregation.MINUTE: ask_data},
        )
        self.wrangler.pre_process(0)

        bar_store = ObjectStorer()
        handler = bar_store.store_2
        symbol = TestStubs.symbol_usdjpy_fxcm()
        bar_spec = BarSpecification(3, BarAggregation.TICK, PriceType.MID)
        bar_type = BarType(symbol, bar_spec)

        clock = TestClock()
        logger = TestLogger(clock)

        ticks = self.wrangler.build_ticks()
        builder = BulkTickBarBuilder(bar_type, logger, handler)

        # Act
        builder.receive(ticks)

        # Assert
        self.assertEqual(333, len(bar_store.get_store()[0][1]))
