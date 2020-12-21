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
from decimal import Decimal
import unittest

from parameterized import parameterized
import pytz

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import TestLogger
from nautilus_trader.data.aggregation import BarBuilder
from nautilus_trader.data.aggregation import BulkTickBarBuilder
from nautilus_trader.data.aggregation import TickBarAggregator
from nautilus_trader.data.aggregation import TimeBarAggregator
from nautilus_trader.data.aggregation import ValueBarAggregator
from nautilus_trader.data.aggregation import VolumeBarAggregator
from nautilus_trader.data.wrangling import QuoteTickDataWrangler
from nautilus_trader.data.wrangling import TradeTickDataWrangler
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.bar import BarType
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import TradeMatchId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.model.tick import TradeTick
from tests.test_kit.mocks import ObjectStorer
from tests.test_kit.providers import TestDataProvider
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs
from tests.test_kit.stubs import UNIX_EPOCH


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy(TestStubs.symbol_audusd_fxcm())
ETHUSDT_BINANCE = TestInstrumentProvider.ethusd_bitmex()


class BarBuilderTests(unittest.TestCase):

    def test_instantiate(self):
        # Arrange
        bar_spec = TestStubs.bar_spec_1min_mid()
        builder = BarBuilder(bar_spec, use_previous_close=False)

        # Act
        # Assert
        self.assertEqual(bar_spec, builder.bar_spec)
        self.assertFalse(builder.use_previous_close)
        self.assertFalse(builder.initialized)
        self.assertIsNone(builder.last_timestamp)
        self.assertEqual(0, builder.count)

    def test_str_repr(self):
        # Arrange
        bar_spec = TestStubs.bar_spec_1min_mid()
        builder = BarBuilder(bar_spec, use_previous_close=False)

        # Act
        # Assert
        self.assertEqual("BarBuilder(bar_spec=1-MINUTE-MID,None,None,None,None,0)", str(builder))
        self.assertEqual("BarBuilder(bar_spec=1-MINUTE-MID,None,None,None,None,0)", repr(builder))

    def test_single_update_results_in_expected_properties(self):
        # Arrange
        bar_spec = TestStubs.bar_spec_1min_mid()
        builder = BarBuilder(bar_spec, use_previous_close=True)

        # Act
        builder.update(Price("1.00000"), Quantity("1"), UNIX_EPOCH)

        # Assert
        self.assertTrue(builder.initialized)
        self.assertEqual(UNIX_EPOCH, builder.last_timestamp)
        self.assertEqual(1, builder.count)

    def test_single_update_when_timestamp_less_than_last_update_ignores(self):
        # Arrange
        bar_spec = TestStubs.bar_spec_1min_mid()
        builder = BarBuilder(bar_spec, use_previous_close=True)
        builder.update(Price("1.00000"), Quantity("1"), UNIX_EPOCH)

        # Act
        builder.update(Price("1.00001"), Quantity("1"), UNIX_EPOCH - timedelta(seconds=1))

        # Assert
        self.assertTrue(builder.initialized)
        self.assertEqual(UNIX_EPOCH, builder.last_timestamp)
        self.assertEqual(1, builder.count)

    def test_multiple_updates_correctly_increments_count(self):
        # Arrange
        bar_spec = TestStubs.bar_spec_1min_mid()
        builder = BarBuilder(bar_spec, use_previous_close=True)

        # Act
        builder.update(Price("1.00000"), Quantity("1"), UNIX_EPOCH)
        builder.update(Price("1.00000"), Quantity("1"), UNIX_EPOCH)
        builder.update(Price("1.00000"), Quantity("1"), UNIX_EPOCH)
        builder.update(Price("1.00000"), Quantity("1"), UNIX_EPOCH)
        builder.update(Price("1.00000"), Quantity("1"), UNIX_EPOCH)

        # Assert
        self.assertEqual(5, builder.count)

    def test_build_when_no_updates_raises_exception(self):
        # Arrange
        bar_spec = TestStubs.bar_spec_1min_mid()
        builder = BarBuilder(bar_spec, use_previous_close=False)

        # Act
        # Assert
        self.assertRaises(TypeError, builder.build)

    def test_build_when_received_updates_returns_expected_bar(self):
        # Arrange
        bar_spec = TestStubs.bar_spec_1min_bid()
        builder = BarBuilder(bar_spec, use_previous_close=True)

        builder.update(Price("1.00001"), Quantity("1.0"), UNIX_EPOCH)
        builder.update(Price("1.00002"), Quantity("1.5"), UNIX_EPOCH)
        builder.update(Price("1.00000"), Quantity("1.5"), UNIX_EPOCH + timedelta(seconds=1))

        # Act
        bar = builder.build()  # Also resets builder

        # Assert
        self.assertEqual(Price("1.00001"), bar.open)
        self.assertEqual(Price("1.00002"), bar.high)
        self.assertEqual(Price("1.00000"), bar.low)
        self.assertEqual(Price("1.00000"), bar.close)
        self.assertEqual(Quantity("4.0"), bar.volume)
        self.assertEqual(UNIX_EPOCH + timedelta(seconds=1), bar.timestamp)
        self.assertEqual(UNIX_EPOCH + timedelta(seconds=1), builder.last_timestamp)
        self.assertEqual(0, builder.count)

    def test_build_with_previous_close(self):
        # Arrange
        bar_spec = TestStubs.bar_spec_1min_mid()
        builder = BarBuilder(bar_spec, use_previous_close=True)

        builder.update(Price("1.00001"), Quantity("1.0"), UNIX_EPOCH)
        builder.build()  # This close should become the next open

        builder.update(Price("1.00000"), Quantity("1.0"), UNIX_EPOCH)
        builder.update(Price("1.00003"), Quantity("1.0"), UNIX_EPOCH)
        builder.update(Price("1.00002"), Quantity("1.0"), UNIX_EPOCH)

        bar2 = builder.build()

        # Assert
        self.assertEqual(Price("1.00001"), bar2.open)
        self.assertEqual(Price("1.00003"), bar2.high)
        self.assertEqual(Price("1.00000"), bar2.low)
        self.assertEqual(Price("1.00002"), bar2.close)
        self.assertEqual(Quantity("3.0"), bar2.volume)


class TickBarAggregatorTests(unittest.TestCase):

    def test_handle_quote_tick_when_count_below_threshold_updates(self):
        # Arrange
        bar_store = ObjectStorer()
        handler = bar_store.store
        symbol = TestStubs.symbol_audusd_fxcm()
        bar_spec = BarSpecification(3, BarAggregation.TICK, PriceType.MID)
        bar_type = BarType(symbol, bar_spec)
        aggregator = TickBarAggregator(bar_type, handler, TestLogger(TestClock()))

        tick1 = QuoteTick(
            symbol=AUDUSD_SIM.symbol,
            bid=Price("1.00001"),
            ask=Price("1.00004"),
            bid_size=Quantity(1),
            ask_size=Quantity(1),
            timestamp=UNIX_EPOCH,
        )

        # Act
        aggregator.handle_quote_tick(tick1)

        # Assert
        self.assertEqual(0, len(bar_store.get_store()))

    def test_handle_trade_tick_when_count_below_threshold_updates(self):
        # Arrange
        bar_store = ObjectStorer()
        handler = bar_store.store
        symbol = TestStubs.symbol_audusd_fxcm()
        bar_spec = BarSpecification(3, BarAggregation.TICK, PriceType.LAST)
        bar_type = BarType(symbol, bar_spec)
        aggregator = TickBarAggregator(bar_type, handler, TestLogger(TestClock()))

        tick1 = TradeTick(
            symbol=AUDUSD_SIM.symbol,
            price=Price("1.00001"),
            size=Quantity(1),
            side=OrderSide.BUY,
            match_id=TradeMatchId("123456"),
            timestamp=UNIX_EPOCH,
        )

        # Act
        aggregator.handle_trade_tick(tick1)

        # Assert
        self.assertEqual(0, len(bar_store.get_store()))

    def test_handle_quote_tick_when_count_at_threshold_sends_bar_to_handler(self):
        # Arrange
        bar_store = ObjectStorer()
        handler = bar_store.store
        symbol = TestStubs.symbol_audusd_fxcm()
        bar_spec = BarSpecification(3, BarAggregation.TICK, PriceType.MID)
        bar_type = BarType(symbol, bar_spec)
        aggregator = TickBarAggregator(bar_type, handler, TestLogger(TestClock()))

        tick1 = QuoteTick(
            symbol=AUDUSD_SIM.symbol,
            bid=Price("1.00001"),
            ask=Price("1.00004"),
            bid_size=Quantity(1),
            ask_size=Quantity(1),
            timestamp=UNIX_EPOCH,
        )

        tick2 = QuoteTick(
            symbol=AUDUSD_SIM.symbol,
            bid=Price("1.00002"),
            ask=Price("1.00005"),
            bid_size=Quantity(1),
            ask_size=Quantity(1),
            timestamp=UNIX_EPOCH,
        )

        tick3 = QuoteTick(
            symbol=AUDUSD_SIM.symbol,
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
        self.assertEqual(Price("1.000025"), bar_store.get_store()[0].bar.open)
        self.assertEqual(Price("1.000035"), bar_store.get_store()[0].bar.high)
        self.assertEqual(Price("1.000015"), bar_store.get_store()[0].bar.low)
        self.assertEqual(Price("1.000015"), bar_store.get_store()[0].bar.close)
        self.assertEqual(Quantity(6), bar_store.get_store()[0].bar.volume)

    def test_handle_trade_tick_when_count_at_threshold_sends_bar_to_handler(self):
        # Arrange
        bar_store = ObjectStorer()
        handler = bar_store.store
        symbol = TestStubs.symbol_audusd_fxcm()
        bar_spec = BarSpecification(3, BarAggregation.TICK, PriceType.LAST)
        bar_type = BarType(symbol, bar_spec)
        aggregator = TickBarAggregator(bar_type, handler, TestLogger(TestClock()))

        tick1 = TradeTick(
            symbol=AUDUSD_SIM.symbol,
            price=Price("1.00001"),
            size=Quantity(1),
            side=OrderSide.BUY,
            match_id=TradeMatchId("123456"),
            timestamp=UNIX_EPOCH,
        )

        tick2 = TradeTick(
            symbol=AUDUSD_SIM.symbol,
            price=Price("1.00002"),
            size=Quantity(1),
            side=OrderSide.BUY,
            match_id=TradeMatchId("123457"),
            timestamp=UNIX_EPOCH,
        )

        tick3 = TradeTick(
            symbol=AUDUSD_SIM.symbol,
            price=Price("1.00000"),
            size=Quantity(1),
            side=OrderSide.BUY,
            match_id=TradeMatchId("123458"),
            timestamp=UNIX_EPOCH,
        )

        # Act
        aggregator.handle_trade_tick(tick1)
        aggregator.handle_trade_tick(tick2)
        aggregator.handle_trade_tick(tick3)

        # Assert
        self.assertEqual(1, len(bar_store.get_store()))
        self.assertEqual(Price("1.00001"), bar_store.get_store()[0].bar.open)
        self.assertEqual(Price("1.00002"), bar_store.get_store()[0].bar.high)
        self.assertEqual(Price("1.00000"), bar_store.get_store()[0].bar.low)
        self.assertEqual(Price("1.00000"), bar_store.get_store()[0].bar.close)
        self.assertEqual(Quantity(3), bar_store.get_store()[0].bar.volume)

    def test_run_quote_ticks_through_aggregator_results_in_expected_bars(self):
        # Arrange
        bar_store = ObjectStorer()
        handler = bar_store.store
        symbol = TestStubs.symbol_audusd_fxcm()
        bar_spec = BarSpecification(100, BarAggregation.TICK, PriceType.MID)
        bar_type = BarType(symbol, bar_spec)
        aggregator = TickBarAggregator(bar_type, handler, TestLogger(TestClock()))

        wrangler = QuoteTickDataWrangler(
            instrument=AUDUSD_SIM,
            data_quotes=TestDataProvider.audusd_ticks(),
        )

        wrangler.pre_process(symbol_indexer=0)
        ticks = wrangler.build_ticks()

        # Act
        for tick in ticks:
            aggregator.handle_quote_tick(tick)

        # Assert
        last_bar = bar_store.get_store()[-1].bar
        self.assertEqual(999, len(bar_store.get_store()))
        self.assertEqual(Price("0.66939"), last_bar.open)
        self.assertEqual(Price("0.66947"), last_bar.high)
        self.assertEqual(Price("0.669355"), last_bar.low)
        self.assertEqual(Price("0.66945"), last_bar.close)
        self.assertEqual(Quantity(200), last_bar.volume)

    def test_run_trade_ticks_through_aggregator_results_in_expected_bars(self):
        # Arrange
        bar_store = ObjectStorer()
        handler = bar_store.store
        bar_spec = BarSpecification(1000, BarAggregation.TICK, PriceType.LAST)
        bar_type = BarType(ETHUSDT_BINANCE.symbol, bar_spec)
        aggregator = TickBarAggregator(bar_type, handler, TestLogger(TestClock()))

        wrangler = TradeTickDataWrangler(
            instrument=ETHUSDT_BINANCE,
            data=TestDataProvider.ethusdt_trades(),
        )

        wrangler.pre_process(0)
        ticks = wrangler.build_ticks()

        # Act
        for tick in ticks:
            aggregator.handle_trade_tick(tick)

        # Assert
        last_bar = bar_store.get_store()[-1].bar
        self.assertEqual(69, len(bar_store.get_store()))
        self.assertEqual(Price("426.72"), last_bar.open)
        self.assertEqual(Price("427.01"), last_bar.high)
        self.assertEqual(Price("426.46"), last_bar.low)
        self.assertEqual(Price("426.67"), last_bar.close)
        self.assertEqual(Quantity(2281), last_bar.volume)


class VolumeBarAggregatorTests(unittest.TestCase):

    def test_handle_quote_tick_when_volume_below_threshold_updates(self):
        # Arrange
        bar_store = ObjectStorer()
        handler = bar_store.store
        symbol = TestStubs.symbol_audusd_fxcm()
        bar_spec = BarSpecification(10000, BarAggregation.VOLUME, PriceType.BID)
        bar_type = BarType(symbol, bar_spec)
        aggregator = VolumeBarAggregator(bar_type, handler, TestLogger(TestClock()))

        tick1 = QuoteTick(
            symbol=AUDUSD_SIM.symbol,
            bid=Price("1.00001"),
            ask=Price("1.00004"),
            bid_size=Quantity(3000),
            ask_size=Quantity(2000),
            timestamp=UNIX_EPOCH,
        )

        # Act
        aggregator.handle_quote_tick(tick1)

        # Assert
        self.assertEqual(0, len(bar_store.get_store()))

    def test_handle_trade_tick_when_volume_below_threshold_updates(self):
        # Arrange
        bar_store = ObjectStorer()
        handler = bar_store.store
        symbol = TestStubs.symbol_audusd_fxcm()
        bar_spec = BarSpecification(10000, BarAggregation.VOLUME, PriceType.LAST)
        bar_type = BarType(symbol, bar_spec)
        aggregator = VolumeBarAggregator(bar_type, handler, TestLogger(TestClock()))

        tick1 = TradeTick(
            symbol=AUDUSD_SIM.symbol,
            price=Price("1.00001"),
            size=Quantity(1),
            side=OrderSide.BUY,
            match_id=TradeMatchId("123456"),
            timestamp=UNIX_EPOCH,
        )

        # Act
        aggregator.handle_trade_tick(tick1)

        # Assert
        self.assertEqual(0, len(bar_store.get_store()))

    def test_handle_quote_tick_when_volume_at_threshold_sends_bar_to_handler(self):
        # Arrange
        bar_store = ObjectStorer()
        handler = bar_store.store
        symbol = TestStubs.symbol_audusd_fxcm()
        bar_spec = BarSpecification(10000, BarAggregation.VOLUME, PriceType.BID)
        bar_type = BarType(symbol, bar_spec)
        aggregator = VolumeBarAggregator(bar_type, handler, TestLogger(TestClock()))

        tick1 = QuoteTick(
            symbol=AUDUSD_SIM.symbol,
            bid=Price("1.00001"),
            ask=Price("1.00004"),
            bid_size=Quantity(3000),
            ask_size=Quantity(2000),
            timestamp=UNIX_EPOCH,
        )

        tick2 = QuoteTick(
            symbol=AUDUSD_SIM.symbol,
            bid=Price("1.00002"),
            ask=Price("1.00005"),
            bid_size=Quantity(4000),
            ask_size=Quantity(2000),
            timestamp=UNIX_EPOCH,
        )

        tick3 = QuoteTick(
            symbol=AUDUSD_SIM.symbol,
            bid=Price("1.00000"),
            ask=Price("1.00003"),
            bid_size=Quantity(3000),
            ask_size=Quantity(2000),
            timestamp=UNIX_EPOCH,
        )

        # Act
        aggregator.handle_quote_tick(tick1)
        aggregator.handle_quote_tick(tick2)
        aggregator.handle_quote_tick(tick3)

        # Assert
        self.assertEqual(1, len(bar_store.get_store()))
        self.assertEqual(Price("1.00001"), bar_store.get_store()[0].bar.open)
        self.assertEqual(Price("1.00002"), bar_store.get_store()[0].bar.high)
        self.assertEqual(Price("1.00000"), bar_store.get_store()[0].bar.low)
        self.assertEqual(Price('1.00000'), bar_store.get_store()[0].bar.close)
        self.assertEqual(Quantity(10000), bar_store.get_store()[0].bar.volume)

    def test_handle_trade_tick_when_volume_at_threshold_sends_bar_to_handler(self):
        # Arrange
        bar_store = ObjectStorer()
        handler = bar_store.store
        symbol = TestStubs.symbol_audusd_fxcm()
        bar_spec = BarSpecification(10000, BarAggregation.VOLUME, PriceType.LAST)
        bar_type = BarType(symbol, bar_spec)
        aggregator = VolumeBarAggregator(bar_type, handler, TestLogger(TestClock()))

        tick1 = TradeTick(
            symbol=AUDUSD_SIM.symbol,
            price=Price("1.00001"),
            size=Quantity(3000),
            side=OrderSide.BUY,
            match_id=TradeMatchId("123456"),
            timestamp=UNIX_EPOCH,
        )

        tick2 = TradeTick(
            symbol=AUDUSD_SIM.symbol,
            price=Price("1.00002"),
            size=Quantity(4000),
            side=OrderSide.BUY,
            match_id=TradeMatchId("123457"),
            timestamp=UNIX_EPOCH,
        )

        tick3 = TradeTick(
            symbol=AUDUSD_SIM.symbol,
            price=Price("1.00000"),
            size=Quantity(3000),
            side=OrderSide.BUY,
            match_id=TradeMatchId("123458"),
            timestamp=UNIX_EPOCH,
        )

        # Act
        aggregator.handle_trade_tick(tick1)
        aggregator.handle_trade_tick(tick2)
        aggregator.handle_trade_tick(tick3)

        # Assert
        self.assertEqual(1, len(bar_store.get_store()))
        self.assertEqual(Price("1.00001"), bar_store.get_store()[0].bar.open)
        self.assertEqual(Price("1.00002"), bar_store.get_store()[0].bar.high)
        self.assertEqual(Price("1.00000"), bar_store.get_store()[0].bar.low)
        self.assertEqual(Price('1.00000'), bar_store.get_store()[0].bar.close)
        self.assertEqual(Quantity(10000), bar_store.get_store()[0].bar.volume)

    def test_handle_quote_tick_when_volume_beyond_threshold_sends_bars_to_handler(self):
        # Arrange
        bar_store = ObjectStorer()
        handler = bar_store.store
        symbol = TestStubs.symbol_audusd_fxcm()
        bar_spec = BarSpecification(10000, BarAggregation.VOLUME, PriceType.BID)
        bar_type = BarType(symbol, bar_spec)
        aggregator = VolumeBarAggregator(bar_type, handler, TestLogger(TestClock()))

        tick1 = QuoteTick(
            symbol=AUDUSD_SIM.symbol,
            bid=Price("1.00001"),
            ask=Price("1.00004"),
            bid_size=Quantity(2000),
            ask_size=Quantity(2000),
            timestamp=UNIX_EPOCH,
        )

        tick2 = QuoteTick(
            symbol=AUDUSD_SIM.symbol,
            bid=Price("1.00002"),
            ask=Price("1.00005"),
            bid_size=Quantity(3000),
            ask_size=Quantity(3000),
            timestamp=UNIX_EPOCH,
        )

        tick3 = QuoteTick(
            symbol=AUDUSD_SIM.symbol,
            bid=Price("1.00000"),
            ask=Price("1.00003"),
            bid_size=Quantity(25000),
            ask_size=Quantity(25000),
            timestamp=UNIX_EPOCH,
        )

        # Act
        aggregator.handle_quote_tick(tick1)
        aggregator.handle_quote_tick(tick2)
        aggregator.handle_quote_tick(tick3)

        # Assert
        self.assertEqual(3, len(bar_store.get_store()))
        self.assertEqual(Price("1.00001"), bar_store.get_store()[0].bar.open)
        self.assertEqual(Price("1.00002"), bar_store.get_store()[0].bar.high)
        self.assertEqual(Price("1.00000"), bar_store.get_store()[0].bar.low)
        self.assertEqual(Price('1.00000'), bar_store.get_store()[0].bar.close)
        self.assertEqual(Quantity(10000), bar_store.get_store()[0].bar.volume)
        self.assertEqual(Price("1.00000"), bar_store.get_store()[1].bar.open)
        self.assertEqual(Price("1.00000"), bar_store.get_store()[1].bar.high)
        self.assertEqual(Price("1.00000"), bar_store.get_store()[1].bar.low)
        self.assertEqual(Price('1.00000'), bar_store.get_store()[1].bar.close)
        self.assertEqual(Quantity(10000), bar_store.get_store()[1].bar.volume)
        self.assertEqual(Price("1.00000"), bar_store.get_store()[2].bar.open)
        self.assertEqual(Price("1.00000"), bar_store.get_store()[2].bar.high)
        self.assertEqual(Price("1.00000"), bar_store.get_store()[2].bar.low)
        self.assertEqual(Price('1.00000'), bar_store.get_store()[2].bar.close)
        self.assertEqual(Quantity(10000), bar_store.get_store()[2].bar.volume)

    def test_handle_trade_tick_when_volume_beyond_threshold_sends_bars_to_handler(self):
        # Arrange
        bar_store = ObjectStorer()
        handler = bar_store.store
        symbol = TestStubs.symbol_audusd_fxcm()
        bar_spec = BarSpecification(10000, BarAggregation.VOLUME, PriceType.LAST)
        bar_type = BarType(symbol, bar_spec)
        aggregator = VolumeBarAggregator(bar_type, handler, TestLogger(TestClock()))

        tick1 = TradeTick(
            symbol=AUDUSD_SIM.symbol,
            price=Price("1.00001"),
            size=Quantity(2000),
            side=OrderSide.BUY,
            match_id=TradeMatchId("123456"),
            timestamp=UNIX_EPOCH,
        )

        tick2 = TradeTick(
            symbol=AUDUSD_SIM.symbol,
            price=Price("1.00002"),
            size=Quantity(3000),
            side=OrderSide.BUY,
            match_id=TradeMatchId("123457"),
            timestamp=UNIX_EPOCH,
        )

        tick3 = TradeTick(
            symbol=AUDUSD_SIM.symbol,
            price=Price("1.00000"),
            size=Quantity(25000),
            side=OrderSide.BUY,
            match_id=TradeMatchId("123458"),
            timestamp=UNIX_EPOCH,
        )

        # Act
        aggregator.handle_trade_tick(tick1)
        aggregator.handle_trade_tick(tick2)
        aggregator.handle_trade_tick(tick3)

        # Assert
        self.assertEqual(3, len(bar_store.get_store()))
        self.assertEqual(Price("1.00001"), bar_store.get_store()[0].bar.open)
        self.assertEqual(Price("1.00002"), bar_store.get_store()[0].bar.high)
        self.assertEqual(Price("1.00000"), bar_store.get_store()[0].bar.low)
        self.assertEqual(Price('1.00000'), bar_store.get_store()[0].bar.close)
        self.assertEqual(Quantity(10000), bar_store.get_store()[0].bar.volume)
        self.assertEqual(Price("1.00000"), bar_store.get_store()[1].bar.open)
        self.assertEqual(Price("1.00000"), bar_store.get_store()[1].bar.high)
        self.assertEqual(Price("1.00000"), bar_store.get_store()[1].bar.low)
        self.assertEqual(Price('1.00000'), bar_store.get_store()[1].bar.close)
        self.assertEqual(Quantity(10000), bar_store.get_store()[1].bar.volume)
        self.assertEqual(Price("1.00000"), bar_store.get_store()[2].bar.open)
        self.assertEqual(Price("1.00000"), bar_store.get_store()[2].bar.high)
        self.assertEqual(Price("1.00000"), bar_store.get_store()[2].bar.low)
        self.assertEqual(Price('1.00000'), bar_store.get_store()[2].bar.close)
        self.assertEqual(Quantity(10000), bar_store.get_store()[2].bar.volume)

    def test_run_quote_ticks_through_aggregator_results_in_expected_bars(self):
        # Arrange
        bar_store = ObjectStorer()
        handler = bar_store.store
        symbol = TestStubs.symbol_audusd_fxcm()
        bar_spec = BarSpecification(1000, BarAggregation.VOLUME, PriceType.MID)
        bar_type = BarType(symbol, bar_spec)
        aggregator = VolumeBarAggregator(bar_type, handler, TestLogger(TestClock()))

        wrangler = QuoteTickDataWrangler(
            instrument=AUDUSD_SIM,
            data_quotes=TestDataProvider.audusd_ticks(),
        )

        wrangler.pre_process(symbol_indexer=0)
        ticks = wrangler.build_ticks()

        # Act
        for tick in ticks:
            aggregator.handle_quote_tick(tick)

        # Assert
        last_bar = bar_store.get_store()[-1].bar
        self.assertEqual(199, len(bar_store.get_store()))
        self.assertEqual(Price("0.669355"), last_bar.open)
        self.assertEqual(Price("0.66939"), last_bar.high)
        self.assertEqual(Price("0.66922"), last_bar.low)
        self.assertEqual(Price("0.66932"), last_bar.close)
        self.assertEqual(Quantity(1000), last_bar.volume)

    def test_run_trade_ticks_through_aggregator_results_in_expected_bars(self):
        # Arrange
        bar_store = ObjectStorer()
        handler = bar_store.store
        bar_spec = BarSpecification(1000, BarAggregation.VOLUME, PriceType.LAST)
        bar_type = BarType(ETHUSDT_BINANCE.symbol, bar_spec)
        aggregator = VolumeBarAggregator(bar_type, handler, TestLogger(TestClock()))

        wrangler = TradeTickDataWrangler(
            instrument=ETHUSDT_BINANCE,
            data=TestDataProvider.ethusdt_trades(),
        )

        wrangler.pre_process(0)
        ticks = wrangler.build_ticks()

        # Act
        for tick in ticks:
            aggregator.handle_trade_tick(tick)

        # Assert
        last_bar = bar_store.get_store()[-1].bar
        self.assertEqual(187, len(bar_store.get_store()))
        self.assertEqual(Price("426.44"), last_bar.open)
        self.assertEqual(Price("426.84"), last_bar.high)
        self.assertEqual(Price("426.00"), last_bar.low)
        self.assertEqual(Price("426.82"), last_bar.close)
        self.assertEqual(Quantity(1000), last_bar.volume)


class ValueBarAggregatorTests(unittest.TestCase):

    def test_handle_quote_tick_when_value_below_threshold_updates(self):
        # Arrange
        bar_store = ObjectStorer()
        handler = bar_store.store
        symbol = TestStubs.symbol_audusd_fxcm()
        bar_spec = BarSpecification(100000, BarAggregation.VALUE, PriceType.BID)
        bar_type = BarType(symbol, bar_spec)
        aggregator = ValueBarAggregator(bar_type, handler, TestLogger(TestClock()))

        tick1 = QuoteTick(
            symbol=AUDUSD_SIM.symbol,
            bid=Price("1.00001"),
            ask=Price("1.00004"),
            bid_size=Quantity(3000),
            ask_size=Quantity(2000),
            timestamp=UNIX_EPOCH,
        )

        # Act
        aggregator.handle_quote_tick(tick1)

        # Assert
        self.assertEqual(0, len(bar_store.get_store()))
        self.assertEqual(Decimal("3000.03000"), aggregator.cum_value)

    def test_handle_trade_tick_when_value_below_threshold_updates(self):
        # Arrange
        bar_store = ObjectStorer()
        handler = bar_store.store
        symbol = TestStubs.symbol_audusd_fxcm()
        bar_spec = BarSpecification(100000, BarAggregation.VALUE, PriceType.LAST)
        bar_type = BarType(symbol, bar_spec)
        aggregator = ValueBarAggregator(bar_type, handler, TestLogger(TestClock()))

        tick1 = TradeTick(
            symbol=AUDUSD_SIM.symbol,
            price=Price("15000.00"),
            size=Quantity("3.5"),
            side=OrderSide.BUY,
            match_id=TradeMatchId("123456"),
            timestamp=UNIX_EPOCH,
        )

        # Act
        aggregator.handle_trade_tick(tick1)

        # Assert
        self.assertEqual(0, len(bar_store.get_store()))
        self.assertEqual(Decimal("52500.000"), aggregator.cum_value)

    def test_handle_quote_tick_when_value_beyond_threshold_sends_bar_to_handler(self):
        # Arrange
        bar_store = ObjectStorer()
        handler = bar_store.store
        symbol = TestStubs.symbol_audusd_fxcm()
        bar_spec = BarSpecification(100000, BarAggregation.VALUE, PriceType.BID)
        bar_type = BarType(symbol, bar_spec)
        aggregator = ValueBarAggregator(bar_type, handler, TestLogger(TestClock()))

        tick1 = QuoteTick(
            symbol=AUDUSD_SIM.symbol,
            bid=Price("1.00001"),
            ask=Price("1.00004"),
            bid_size=Quantity(20000),
            ask_size=Quantity(20000),
            timestamp=UNIX_EPOCH,
        )

        tick2 = QuoteTick(
            symbol=AUDUSD_SIM.symbol,
            bid=Price("1.00002"),
            ask=Price("1.00005"),
            bid_size=Quantity(60000),
            ask_size=Quantity(20000),
            timestamp=UNIX_EPOCH,
        )

        tick3 = QuoteTick(
            symbol=AUDUSD_SIM.symbol,
            bid=Price("1.00000"),
            ask=Price("1.00003"),
            bid_size=Quantity(30500),
            ask_size=Quantity(20000),
            timestamp=UNIX_EPOCH,
        )

        # Act
        aggregator.handle_quote_tick(tick1)
        aggregator.handle_quote_tick(tick2)
        aggregator.handle_quote_tick(tick3)

        # Assert
        self.assertEqual(1, len(bar_store.get_store()))
        self.assertEqual(Price("1.00001"), bar_store.get_store()[0].bar.open)
        self.assertEqual(Price("1.00002"), bar_store.get_store()[0].bar.high)
        self.assertEqual(Price("1.00000"), bar_store.get_store()[0].bar.low)
        self.assertEqual(Price('1.00000'), bar_store.get_store()[0].bar.close)
        self.assertEqual(Quantity("99999"), bar_store.get_store()[0].bar.volume)
        self.assertEqual(Decimal("10501.00000"), aggregator.cum_value)

    def test_handle_trade_tick_when_volume_beyond_threshold_sends_bars_to_handler(self):
        # Arrange
        bar_store = ObjectStorer()
        handler = bar_store.store
        symbol = TestStubs.symbol_audusd_fxcm()
        bar_spec = BarSpecification(100000, BarAggregation.VALUE, PriceType.LAST)
        bar_type = BarType(symbol, bar_spec)
        aggregator = ValueBarAggregator(bar_type, handler, TestLogger(TestClock()))

        tick1 = TradeTick(
            symbol=AUDUSD_SIM.symbol,
            price=Price("20.00001"),
            size=Quantity("3000.00"),
            side=OrderSide.BUY,
            match_id=TradeMatchId("123456"),
            timestamp=UNIX_EPOCH,
        )

        tick2 = TradeTick(
            symbol=AUDUSD_SIM.symbol,
            price=Price("20.00002"),
            size=Quantity("4000.00"),
            side=OrderSide.BUY,
            match_id=TradeMatchId("123457"),
            timestamp=UNIX_EPOCH,
        )

        tick3 = TradeTick(
            symbol=AUDUSD_SIM.symbol,
            price=Price("20.00000"),
            size=Quantity("5000.00"),
            side=OrderSide.BUY,
            match_id=TradeMatchId("123458"),
            timestamp=UNIX_EPOCH,
        )

        # Act
        aggregator.handle_trade_tick(tick1)
        aggregator.handle_trade_tick(tick2)
        aggregator.handle_trade_tick(tick3)

        # Assert
        self.assertEqual(2, len(bar_store.get_store()))
        self.assertEqual(Price("20.00001"), bar_store.get_store()[0].bar.open)
        self.assertEqual(Price("20.00002"), bar_store.get_store()[0].bar.high)
        self.assertEqual(Price("20.00001"), bar_store.get_store()[0].bar.low)
        self.assertEqual(Price('20.00002'), bar_store.get_store()[0].bar.close)
        self.assertEqual(Quantity("5000"), bar_store.get_store()[0].bar.volume)
        self.assertEqual(Price("20.00002"), bar_store.get_store()[1].bar.open)
        self.assertEqual(Price("20.00002"), bar_store.get_store()[1].bar.high)
        self.assertEqual(Price("20.00000"), bar_store.get_store()[1].bar.low)
        self.assertEqual(Price('20.00000'), bar_store.get_store()[1].bar.close)
        self.assertEqual(Quantity("5000.00"), bar_store.get_store()[1].bar.volume)
        self.assertEqual(Decimal("40000.00000"), aggregator.cum_value)

    def test_run_quote_ticks_through_aggregator_results_in_expected_bars(self):
        # Arrange
        bar_store = ObjectStorer()
        handler = bar_store.store
        symbol = TestStubs.symbol_audusd_fxcm()
        bar_spec = BarSpecification(1000, BarAggregation.VALUE, PriceType.MID)
        bar_type = BarType(symbol, bar_spec)
        aggregator = ValueBarAggregator(bar_type, handler, TestLogger(TestClock()))

        wrangler = QuoteTickDataWrangler(
            instrument=AUDUSD_SIM,
            data_quotes=TestDataProvider.audusd_ticks(),
        )

        wrangler.pre_process(symbol_indexer=0)
        ticks = wrangler.build_ticks()

        # Act
        for tick in ticks:
            aggregator.handle_quote_tick(tick)

        # Assert
        last_bar = bar_store.get_store()[-1].bar
        self.assertEqual(134, len(bar_store.get_store()))
        self.assertEqual(Price("0.669355"), last_bar.open)
        self.assertEqual(Price("0.66948"), last_bar.high)
        self.assertEqual(Price("0.66922"), last_bar.low)
        self.assertEqual(Price("0.669465"), last_bar.close)
        self.assertEqual(Quantity(1494), last_bar.volume)

    def test_run_trade_ticks_through_aggregator_results_in_expected_bars(self):
        # Arrange
        bar_store = ObjectStorer()
        handler = bar_store.store
        bar_spec = BarSpecification(10000, BarAggregation.VALUE, PriceType.LAST)
        bar_type = BarType(ETHUSDT_BINANCE.symbol, bar_spec)
        aggregator = ValueBarAggregator(bar_type, handler, TestLogger(TestClock()))

        wrangler = TradeTickDataWrangler(
            instrument=ETHUSDT_BINANCE,
            data=TestDataProvider.ethusdt_trades(),
        )

        wrangler.pre_process(0)
        ticks = wrangler.build_ticks()

        # Act
        for tick in ticks:
            aggregator.handle_trade_tick(tick)

        # Assert
        last_bar = bar_store.get_store()[-1].bar
        self.assertEqual(7962, len(bar_store.get_store()))
        self.assertEqual(Price("426.86"), last_bar.open)
        self.assertEqual(Price("426.94"), last_bar.high)
        self.assertEqual(Price("426.83"), last_bar.low)
        self.assertEqual(Price("426.94"), last_bar.close)
        self.assertEqual(Quantity(23), last_bar.volume)


class TimeBarAggregatorTests(unittest.TestCase):

    def test_instantiate_given_invalid_bar_spec_raises_value_error(self):
        # Arrange
        clock = TestClock()
        bar_store = ObjectStorer()
        handler = bar_store.store
        symbol = TestStubs.symbol_audusd_fxcm()
        bar_spec = BarSpecification(100, BarAggregation.TICK, PriceType.MID)
        bar_type = BarType(symbol, bar_spec)

        # Act
        # Assert
        self.assertRaises(
            ValueError,
            TimeBarAggregator,
            bar_type,
            handler,
            True,
            clock,
            TestLogger(clock),
        )

    @parameterized.expand([
        [BarSpecification(10, BarAggregation.SECOND, PriceType.MID), datetime(1970, 1, 1, 0, 0, 10, tzinfo=pytz.utc)],
        [BarSpecification(1, BarAggregation.MINUTE, PriceType.MID), datetime(1970, 1, 1, 0, 1, tzinfo=pytz.utc)],
        [BarSpecification(1, BarAggregation.HOUR, PriceType.MID), datetime(1970, 1, 1, 1, 0, tzinfo=pytz.utc)],
        [BarSpecification(1, BarAggregation.DAY, PriceType.MID), datetime(1970, 1, 2, 0, 0, tzinfo=pytz.utc)],
    ])
    def test_instantiate_with_various_bar_specs(self, bar_spec, expected):
        # Arrange
        clock = TestClock()
        bar_store = ObjectStorer()
        handler = bar_store.store
        symbol = TestStubs.symbol_audusd_fxcm()
        bar_type = BarType(symbol, bar_spec)

        # Act
        aggregator = TimeBarAggregator(bar_type, handler, True, clock, TestLogger(clock))

        # Assert
        self.assertEqual(expected, aggregator.next_close)

    def test_update_timed_with_test_clock_sends_single_bar_to_handler(self):
        # Arrange
        clock = TestClock()
        bar_store = ObjectStorer()
        handler = bar_store.store
        symbol = TestStubs.symbol_audusd_fxcm()
        bar_spec = BarSpecification(1, BarAggregation.MINUTE, PriceType.MID)
        bar_type = BarType(symbol, bar_spec)
        aggregator = TimeBarAggregator(bar_type, handler, True, TestClock(), TestLogger(clock))

        stop_time = UNIX_EPOCH + timedelta(minutes=2)

        tick1 = QuoteTick(
            symbol=AUDUSD_SIM.symbol,
            bid=Price("1.00001"),
            ask=Price("1.00004"),
            bid_size=Quantity(1),
            ask_size=Quantity(1),
            timestamp=UNIX_EPOCH,
        )

        tick2 = QuoteTick(
            symbol=AUDUSD_SIM.symbol,
            bid=Price("1.00002"),
            ask=Price("1.00005"),
            bid_size=Quantity(1),
            ask_size=Quantity(1),
            timestamp=UNIX_EPOCH,
        )

        tick3 = QuoteTick(
            symbol=AUDUSD_SIM.symbol,
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
        self.assertEqual(Price("1.000025"), bar_store.get_store()[0].bar.open)
        self.assertEqual(Price("1.000035"), bar_store.get_store()[0].bar.high)
        self.assertEqual(Price("1.000025"), bar_store.get_store()[0].bar.low)
        self.assertEqual(Price("1.000035"), bar_store.get_store()[0].bar.close)
        self.assertEqual(Quantity(4), bar_store.get_store()[0].bar.volume)
        self.assertEqual(datetime(1970, 1, 1, 0, 1, tzinfo=pytz.utc), bar_store.get_store()[0].bar.timestamp)


class BulkTickBarBuilderTests(unittest.TestCase):

    def test_given_list_of_ticks_aggregates_tick_bars(self):
        # Arrange
        tick_data = TestDataProvider.usdjpy_ticks()
        bid_data = TestDataProvider.usdjpy_1min_bid()
        ask_data = TestDataProvider.usdjpy_1min_ask()
        self.wrangler = QuoteTickDataWrangler(
            instrument=TestInstrumentProvider.default_fx_ccy(TestStubs.symbol_usdjpy_fxcm()),
            data_quotes=tick_data,
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
