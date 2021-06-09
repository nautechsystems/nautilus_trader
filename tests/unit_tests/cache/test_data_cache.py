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

from decimal import Decimal
import unittest

from parameterized import parameterized

from nautilus_trader.model.bar import Bar
from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.currencies import JPY
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import OrderBookLevel
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import TradeMatchId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.book import L2OrderBook
from nautilus_trader.model.orderbook.book import OrderBookSnapshot
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.model.tick import TradeTick
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


SIM = Venue("SIM")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class CacheTests(unittest.TestCase):
    def setUp(self):
        # Fixture Setup
        self.cache = TestStubs.cache()

    def test_reset_an_empty_cache(self):
        # Arrange
        # Act
        self.cache.reset()

        # Assert
        self.assertEqual([], self.cache.instruments())
        self.assertEqual([], self.cache.quote_ticks(AUDUSD_SIM.id))
        self.assertEqual([], self.cache.trade_ticks(AUDUSD_SIM.id))
        self.assertEqual([], self.cache.bars(TestStubs.bartype_gbpusd_1sec_mid()))

    def test_instrument_ids_when_no_instruments_returns_empty_list(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual([], self.cache.instrument_ids())

    def test_instruments_when_no_instruments_returns_empty_list(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual([], self.cache.instruments())

    def test_quote_ticks_for_unknown_instrument_returns_empty_list(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual([], self.cache.quote_ticks(AUDUSD_SIM.id))

    def test_trade_ticks_for_unknown_instrument_returns_empty_list(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual([], self.cache.trade_ticks(AUDUSD_SIM.id))

    def test_bars_for_unknown_bar_type_returns_empty_list(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual([], self.cache.bars(TestStubs.bartype_gbpusd_1sec_mid()))

    def test_instrument_when_no_instruments_returns_none(self):
        # Arrange
        # Act
        # Assert
        self.assertIsNone(self.cache.instrument(AUDUSD_SIM.id))

    def test_order_book_for_unknown_instrument_returns_none(self):
        # Arrange
        # Act
        # Assert
        self.assertIsNone(self.cache.order_book(AUDUSD_SIM.id))

    def test_quote_tick_when_no_ticks_returns_none(self):
        # Arrange
        # Act
        # Assert
        self.assertIsNone(self.cache.quote_tick(AUDUSD_SIM.id))

    def test_trade_tick_when_no_ticks_returns_none(self):
        # Arrange
        # Act
        # Assert
        self.assertIsNone(self.cache.trade_tick(AUDUSD_SIM.id))

    def test_bar_when_no_bars_returns_none(self):
        # Arrange
        # Act
        # Assert
        self.assertIsNone(self.cache.bar(TestStubs.bartype_gbpusd_1sec_mid()))

    def test_quote_tick_count_for_unknown_instrument_returns_zero(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual(0, self.cache.quote_tick_count(AUDUSD_SIM.id))

    def test_trade_tick_count_for_unknown_instrument_returns_zero(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual(0, self.cache.trade_tick_count(AUDUSD_SIM.id))

    def test_has_order_book_for_unknown_instrument_returns_false(self):
        # Arrange
        # Act
        # Assert
        self.assertFalse(self.cache.has_order_book(AUDUSD_SIM.id))

    def test_has_quote_ticks_for_unknown_instrument_returns_false(self):
        # Arrange
        # Act
        # Assert
        self.assertFalse(self.cache.has_quote_ticks(AUDUSD_SIM.id))

    def test_has_trade_ticks_for_unknown_instrument_returns_false(self):
        # Arrange
        # Act
        # Assert
        self.assertFalse(self.cache.has_trade_ticks(AUDUSD_SIM.id))

    def test_has_bars_for_unknown_bar_type_returns_false(self):
        # Arrange
        # Act
        # Assert
        self.assertFalse(self.cache.has_bars(TestStubs.bartype_gbpusd_1sec_mid()))

    def test_instrument_ids_when_one_instrument_returns_expected_list(self):
        # Arrange
        instrument = TestInstrumentProvider.ethusdt_binance()

        self.cache.add_instrument(instrument)

        # Act
        result = self.cache.instrument_ids()

        # Assert
        self.assertEqual([instrument.id], result)

    def test_instrument_ids_given_same_venue_returns_expected_list(self):
        # Arrange
        instrument = TestInstrumentProvider.ethusdt_binance()

        self.cache.add_instrument(instrument)

        # Act
        result = self.cache.instrument_ids(venue=instrument.venue)

        # Assert
        self.assertEqual([instrument.id], result)

    def test_instrument_ids_given_different_venue_returns_empty_list(self):
        # Arrange
        instrument = TestInstrumentProvider.ethusdt_binance()

        self.cache.add_instrument(instrument)

        # Act
        result = self.cache.instrument_ids(venue=SIM)

        # Assert
        self.assertEqual([], result)

    def test_instruments_when_one_instrument_returns_expected_list(self):
        # Arrange
        instrument = TestInstrumentProvider.ethusdt_binance()

        self.cache.add_instrument(instrument)

        # Act
        result = self.cache.instruments()

        # Assert
        self.assertEqual([instrument], result)

    def test_instruments_given_same_venue_returns_expected_list(self):
        # Arrange
        instrument = TestInstrumentProvider.ethusdt_binance()

        self.cache.add_instrument(instrument)

        # Act
        result = self.cache.instruments(venue=instrument.venue)

        # Assert
        self.assertEqual([instrument], result)

    def test_instruments_given_different_venue_returns_empty_list(self):
        # Arrange
        instrument = TestInstrumentProvider.ethusdt_binance()

        self.cache.add_instrument(instrument)

        # Act
        result = self.cache.instruments(venue=SIM)

        # Assert
        self.assertEqual([], result)

    def test_quote_ticks_when_one_tick_returns_expected_list(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Price.from_str("1.00001"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            0,
            0,
        )

        self.cache.add_quote_ticks([tick])

        # Act
        result = self.cache.quote_ticks(tick.instrument_id)

        # Assert
        self.assertEqual([tick], result)

    def test_add_quote_ticks_when_already_ticks_does_not_add(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Price.from_str("1.00001"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            0,
            0,
        )

        self.cache.add_quote_tick(tick)

        # Act
        self.cache.add_quote_ticks([tick])
        result = self.cache.quote_ticks(tick.instrument_id)

        # Assert
        self.assertEqual([tick], result)

    def test_trade_ticks_when_one_tick_returns_expected_list(self):
        # Arrange
        tick = TradeTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Quantity.from_int(10000),
            AggressorSide.BUY,
            TradeMatchId("123456789"),
            0,
            0,
        )

        self.cache.add_trade_ticks([tick])

        # Act
        result = self.cache.trade_ticks(tick.instrument_id)

        # Assert
        self.assertEqual([tick], result)

    def test_add_trade_ticks_when_already_ticks_does_not_add(self):
        # Arrange
        tick = TradeTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Quantity.from_int(10000),
            AggressorSide.BUY,
            TradeMatchId("123456789"),
            0,
            0,
        )

        self.cache.add_trade_tick(tick)

        # Act
        self.cache.add_trade_ticks([tick])
        result = self.cache.trade_ticks(tick.instrument_id)

        # Assert
        self.assertEqual([tick], result)

    def test_bars_when_one_bar_returns_expected_list(self):
        # Arrange
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        bar = Bar(
            TestStubs.bartype_audusd_1min_bid(),
            Price.from_str("1.00001"),
            Price.from_str("1.00004"),
            Price.from_str("1.00002"),
            Price.from_str("1.00003"),
            Quantity.from_int(100000),
            0,
            0,
        )

        self.cache.add_bars([bar])

        # Act
        result = self.cache.bars(bar_type)

        # Assert
        self.assertTrue([bar], result)

    def test_add_bars_when_already_bars_does_not_add(self):
        # Arrange
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        bar = Bar(
            TestStubs.bartype_audusd_1min_bid(),
            Price.from_str("1.00001"),
            Price.from_str("1.00004"),
            Price.from_str("1.00002"),
            Price.from_str("1.00003"),
            Quantity.from_int(100000),
            0,
            0,
        )

        self.cache.add_bar(bar)

        # Act
        self.cache.add_bars([bar])
        result = self.cache.bars(bar_type)

        # Assert
        self.assertTrue([bar], result)

    def test_instrument_when_no_instrument_returns_none(self):
        # Arrange
        # Act
        result = self.cache.instrument(AUDUSD_SIM.id)

        # Assert
        self.assertIsNone(result)

    def test_instrument_when_instrument_exists_returns_expected(self):
        # Arrange
        self.cache.add_instrument(AUDUSD_SIM)

        # Act
        result = self.cache.instrument(AUDUSD_SIM.id)

        # Assert
        self.assertEqual(AUDUSD_SIM, result)

    def test_order_book_when_order_book_exists_returns_expected(self):
        # Arrange
        snapshot = OrderBookSnapshot(
            instrument_id=ETHUSDT_BINANCE.id,
            level=OrderBookLevel.L2,
            bids=[[1550.15, 0.51], [1580.00, 1.20]],
            asks=[[1552.15, 1.51], [1582.00, 2.20]],
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        order_book = L2OrderBook(
            instrument_id=ETHUSDT_BINANCE.id,
            price_precision=2,
            size_precision=8,
        )
        order_book.apply_snapshot(snapshot)

        self.cache.add_order_book(order_book)

        # Act
        result = self.cache.order_book(ETHUSDT_BINANCE.id)

        # Assert
        self.assertEqual(order_book, result)

    def test_price_when_no_ticks_returns_none(self):
        # Act
        result = self.cache.price(AUDUSD_SIM.id, PriceType.LAST)

        # Assert
        self.assertIsNone(result)

    def test_price_given_last_when_no_trade_ticks_returns_none(self):
        # Act
        tick = QuoteTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Price.from_str("1.00001"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            0,
            0,
        )

        self.cache.add_quote_tick(tick)

        result = self.cache.price(AUDUSD_SIM.id, PriceType.LAST)

        # Assert
        self.assertIsNone(result)

    def test_price_given_quote_price_type_when_no_quote_ticks_returns_none(self):
        # Arrange
        tick = TradeTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Quantity.from_int(10000),
            AggressorSide.BUY,
            TradeMatchId("123456789"),
            0,
            0,
        )

        self.cache.add_trade_tick(tick)

        # Act
        result = self.cache.price(AUDUSD_SIM.id, PriceType.MID)

        # Assert
        self.assertIsNone(result)

    def test_price_given_last_when_trade_tick_returns_expected_price(self):
        # Arrange
        tick = TradeTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Quantity.from_int(10000),
            AggressorSide.BUY,
            TradeMatchId("123456789"),
            0,
            0,
        )

        self.cache.add_trade_tick(tick)

        # Act
        result = self.cache.price(AUDUSD_SIM.id, PriceType.LAST)

        # Assert
        self.assertEqual(Price.from_str("1.00000"), result)

    @parameterized.expand(
        [
            [PriceType.BID, Price.from_str("1.00000")],
            [PriceType.ASK, Price.from_str("1.00001")],
            [PriceType.MID, Price.from_str("1.000005")],
        ]
    )
    def test_price_given_various_quote_price_types_when_quote_tick_returns_expected_price(
        self, price_type, expected
    ):
        # Arrange
        tick = QuoteTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Price.from_str("1.00001"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            0,
            0,
        )

        self.cache.add_quote_tick(tick)

        # Act
        result = self.cache.price(AUDUSD_SIM.id, price_type)

        # Assert
        self.assertEqual(expected, result)

    def test_quote_tick_when_index_out_of_range_returns_none(self):
        # Arrange
        tick = QuoteTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Price.from_str("1.00001"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            0,
            0,
        )

        self.cache.add_quote_tick(tick)

        # Act
        result = self.cache.quote_tick(AUDUSD_SIM.id, index=1)

        # Assert
        self.assertEqual(1, self.cache.quote_tick_count(AUDUSD_SIM.id))
        self.assertIsNone(result)

    def test_quote_tick_with_two_ticks_returns_expected_tick(self):
        # Arrange
        tick1 = QuoteTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Price.from_str("1.00001"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            0,
            0,
        )

        tick2 = QuoteTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00001"),
            Price.from_str("1.00003"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            0,
            0,
        )

        self.cache.add_quote_tick(tick1)
        self.cache.add_quote_tick(tick2)

        # Act
        result = self.cache.quote_tick(AUDUSD_SIM.id, index=0)

        # Assert
        self.assertEqual(2, self.cache.quote_tick_count(AUDUSD_SIM.id))
        self.assertEqual(tick2, result)

    def test_trade_tick_when_index_out_of_range_returns_none(self):
        # Arrange
        tick = TradeTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Quantity.from_int(10000),
            AggressorSide.BUY,
            TradeMatchId("123456789"),
            0,
            0,
        )

        self.cache.add_trade_tick(tick)

        # Act
        result = self.cache.trade_tick(AUDUSD_SIM.id, index=1)

        # Assert
        self.assertEqual(1, self.cache.trade_tick_count(AUDUSD_SIM.id))
        self.assertIsNone(result)

    def test_trade_tick_with_one_tick_returns_expected_tick(self):
        # Arrange
        tick1 = TradeTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00000"),
            Quantity.from_int(10000),
            AggressorSide.BUY,
            TradeMatchId("123456789"),
            0,
            0,
        )

        tick2 = TradeTick(
            AUDUSD_SIM.id,
            Price.from_str("1.00001"),
            Quantity.from_int(20000),
            AggressorSide.SELL,
            TradeMatchId("123456789"),
            0,
            0,
        )

        self.cache.add_trade_tick(tick1)
        self.cache.add_trade_tick(tick2)

        # Act
        result = self.cache.trade_tick(AUDUSD_SIM.id, index=0)

        # Assert
        self.assertEqual(2, self.cache.trade_tick_count(AUDUSD_SIM.id))
        self.assertEqual(tick2, result)

    def test_bar_index_out_of_range_returns_expected_bar(self):
        # Arrange
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        bar = Bar(
            bar_type,
            Price.from_str("1.00001"),
            Price.from_str("1.00004"),
            Price.from_str("1.00002"),
            Price.from_str("1.00003"),
            Quantity.from_int(100000),
            0,
            0,
        )

        self.cache.add_bar(bar)

        # Act
        result = self.cache.bar(bar_type, index=1)

        # Assert
        self.assertEqual(1, self.cache.bar_count(bar_type))
        self.assertIsNone(result)

    def test_bar_with_two_bars_returns_expected_bar(self):
        # Arrange
        bar_type = TestStubs.bartype_gbpusd_1sec_mid()
        bar1 = Bar(
            bar_type,
            Price.from_str("1.00001"),
            Price.from_str("1.00004"),
            Price.from_str("1.00002"),
            Price.from_str("1.00003"),
            Quantity.from_int(100000),
            0,
            0,
        )

        bar2 = Bar(
            bar_type,
            Price.from_str("1.00002"),
            Price.from_str("1.00003"),
            Price.from_str("1.00004"),
            Price.from_str("1.00005"),
            Quantity.from_int(200000),
            0,
            0,
        )

        self.cache.add_bar(bar1)
        self.cache.add_bar(bar2)

        # Act
        result = self.cache.bar(bar_type, index=0)

        # Assert
        self.assertEqual(2, self.cache.bar_count(bar_type))
        self.assertEqual(bar2, result)

    def test_get_xrate_returns_correct_rate(self):
        # Arrange
        self.cache.add_instrument(USDJPY_SIM)

        tick = QuoteTick(
            USDJPY_SIM.id,
            Price.from_str("110.80000"),
            Price.from_str("110.80010"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            0,
            0,
        )

        self.cache.add_quote_tick(tick)

        # Act
        result = self.cache.get_xrate(SIM, JPY, USD)

        # Assert
        self.assertEqual(Decimal("0.009025266685348968705339031887"), result)

    def test_get_xrate_with_no_conversion_returns_one(self):
        # Arrange
        # Act
        result = self.cache.get_xrate(SIM, AUD, AUD)

        # Assert
        self.assertEqual(Decimal("1"), result)

    def test_get_xrate_with_conversion(self):
        # Arrange
        self.cache.add_instrument(AUDUSD_SIM)

        tick = QuoteTick(
            AUDUSD_SIM.id,
            Price.from_str("0.80000"),
            Price.from_str("0.80010"),
            Quantity.from_int(1),
            Quantity.from_int(1),
            0,
            0,
        )

        self.cache.add_quote_tick(tick)

        # Act
        result = self.cache.get_xrate(SIM, AUD, USD)

        # Assert
        self.assertEqual(Decimal("0.80005"), result)
