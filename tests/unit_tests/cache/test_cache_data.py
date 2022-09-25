# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import pytest

from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.currencies import JPY
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.book import L2OrderBook
from nautilus_trader.model.orderbook.data import OrderBookSnapshot
from tests.test_kit.stubs.component import TestComponentStubs
from tests.test_kit.stubs.data import TestDataStubs


SIM = Venue("SIM")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class TestCache:
    def setup(self):
        # Fixture Setup
        self.cache = TestComponentStubs.cache()

    def test_reset_an_empty_cache(self):
        # Arrange, Act
        self.cache.reset()

        # Assert
        assert self.cache.instruments() == []
        assert self.cache.quote_ticks(AUDUSD_SIM.id) == []
        assert self.cache.trade_ticks(AUDUSD_SIM.id) == []
        assert self.cache.bars(TestDataStubs.bartype_gbpusd_1sec_mid()) == []

    def test_instrument_ids_when_no_instruments_returns_empty_list(self):
        # Arrange, Act, Assert
        assert self.cache.instrument_ids() == []

    def test_instruments_when_no_instruments_returns_empty_list(self):
        # Arrange, Act, Assert
        assert self.cache.instruments() == []

    def test_tickers_for_unknown_instrument_returns_empty_list(self):
        # Arrange, Act, Assert
        assert self.cache.tickers(AUDUSD_SIM.id) == []

    def test_quote_ticks_for_unknown_instrument_returns_empty_list(self):
        # Arrange, Act, Assert
        assert self.cache.quote_ticks(AUDUSD_SIM.id) == []

    def test_trade_ticks_for_unknown_instrument_returns_empty_list(self):
        # Arrange, Act, Assert
        assert self.cache.trade_ticks(AUDUSD_SIM.id) == []

    def test_bars_for_unknown_bar_type_returns_empty_list(self):
        # Arrange, Act, Assert
        assert self.cache.bars(TestDataStubs.bartype_gbpusd_1sec_mid()) == []

    def test_instrument_when_no_instruments_returns_none(self):
        # Arrange, Act, Assert
        assert self.cache.instrument(AUDUSD_SIM.id) is None

    def test_order_book_for_unknown_instrument_returns_none(self):
        # Arrange, Act, Assert
        assert self.cache.order_book(AUDUSD_SIM.id) is None

    def test_ticker_when_no_tickers_returns_none(self):
        # Arrange, Act, Assert
        assert self.cache.ticker(AUDUSD_SIM.id) is None

    def test_quote_tick_when_no_ticks_returns_none(self):
        # Arrange, Act, Assert
        assert self.cache.quote_tick(AUDUSD_SIM.id) is None

    def test_trade_tick_when_no_ticks_returns_none(self):
        # Arrange, Act, Assert
        assert self.cache.trade_tick(AUDUSD_SIM.id) is None

    def test_bar_when_no_bars_returns_none(self):
        # Arrange, Act, Assert
        assert self.cache.bar(TestDataStubs.bartype_gbpusd_1sec_mid()) is None

    def test_ticker_count_for_unknown_instrument_returns_zero(self):
        # Arrange, Act, Assert
        assert self.cache.ticker_count(AUDUSD_SIM.id) == 0

    def test_quote_tick_count_for_unknown_instrument_returns_zero(self):
        # Arrange, Act, Assert
        assert self.cache.quote_tick_count(AUDUSD_SIM.id) == 0

    def test_trade_tick_count_for_unknown_instrument_returns_zero(self):
        # Arrange, Act, Assert
        assert self.cache.trade_tick_count(AUDUSD_SIM.id) == 0

    def test_has_order_book_for_unknown_instrument_returns_false(self):
        # Arrange, Act, Assert
        assert not self.cache.has_order_book(AUDUSD_SIM.id)

    def test_has_tickers_for_unknown_instrument_returns_false(self):
        # Arrange, Act, Assert
        assert not self.cache.has_tickers(AUDUSD_SIM.id)

    def test_has_quote_ticks_for_unknown_instrument_returns_false(self):
        # Arrange, Act, Assert
        assert not self.cache.has_quote_ticks(AUDUSD_SIM.id)

    def test_has_trade_ticks_for_unknown_instrument_returns_false(self):
        # Arrange, Act, Assert
        assert not self.cache.has_trade_ticks(AUDUSD_SIM.id)

    def test_has_bars_for_unknown_bar_type_returns_false(self):
        # Arrange, Act, Assert
        assert not self.cache.has_bars(TestDataStubs.bartype_gbpusd_1sec_mid())

    def test_instrument_ids_when_one_instrument_returns_expected_list(self):
        # Arrange
        instrument = TestInstrumentProvider.ethusdt_binance()

        self.cache.add_instrument(instrument)

        # Act
        result = self.cache.instrument_ids()

        # Assert
        assert result == [instrument.id]

    def test_instrument_ids_given_same_venue_returns_expected_list(self):
        # Arrange
        instrument = TestInstrumentProvider.ethusdt_binance()

        self.cache.add_instrument(instrument)

        # Act
        result = self.cache.instrument_ids(venue=instrument.venue)

        # Assert
        assert result == [instrument.id]

    def test_instrument_ids_given_different_venue_returns_empty_list(self):
        # Arrange
        instrument = TestInstrumentProvider.ethusdt_binance()

        self.cache.add_instrument(instrument)

        # Act
        result = self.cache.instrument_ids(venue=SIM)

        # Assert
        assert result == []

    def test_instruments_when_one_instrument_returns_expected_list(self):
        # Arrange
        instrument = TestInstrumentProvider.ethusdt_binance()

        self.cache.add_instrument(instrument)

        # Act
        result = self.cache.instruments()

        # Assert
        assert result == [instrument]

    def test_instruments_given_same_venue_returns_expected_list(self):
        # Arrange
        instrument = TestInstrumentProvider.ethusdt_binance()

        self.cache.add_instrument(instrument)

        # Act
        result = self.cache.instruments(venue=instrument.venue)

        # Assert
        assert result == [instrument]

    def test_instruments_given_different_venue_returns_empty_list(self):
        # Arrange
        instrument = TestInstrumentProvider.ethusdt_binance()

        self.cache.add_instrument(instrument)

        # Act
        result = self.cache.instruments(venue=SIM)

        # Assert
        assert result == []

    def test_quote_ticks_when_one_tick_returns_expected_list(self):
        # Arrange
        tick = TestDataStubs.quote_tick_5decimal()

        self.cache.add_quote_ticks([tick])

        # Act
        result = self.cache.quote_ticks(tick.instrument_id)

        # Assert
        assert result == [tick]

    def test_add_quote_ticks_when_already_ticks_does_not_add(self):
        # Arrange
        tick = TestDataStubs.quote_tick_5decimal()

        self.cache.add_quote_tick(tick)

        # Act
        self.cache.add_quote_ticks([tick])
        result = self.cache.quote_ticks(tick.instrument_id)

        # Assert
        assert result == [tick]

    def test_trade_ticks_when_one_tick_returns_expected_list(self):
        # Arrange
        tick = TestDataStubs.trade_tick_5decimal()

        self.cache.add_trade_ticks([tick])

        # Act
        result = self.cache.trade_ticks(tick.instrument_id)

        # Assert
        assert result == [tick]

    def test_add_trade_ticks_when_already_ticks_does_not_add(self):
        # Arrange
        tick = TestDataStubs.trade_tick_5decimal()

        self.cache.add_trade_tick(tick)

        # Act
        self.cache.add_trade_ticks([tick])
        result = self.cache.trade_ticks(tick.instrument_id)

        # Assert
        assert result == [tick]

    def test_bars_when_one_bar_returns_expected_list(self):
        # Arrange
        bar = TestDataStubs.bar_5decimal()

        self.cache.add_bars([bar])

        # Act
        result = self.cache.bars(bar.bar_type)

        # Assert
        assert result == [bar]

    def test_add_bars_when_already_bars_does_not_add(self):
        # Arrange
        bar = TestDataStubs.bar_5decimal()

        self.cache.add_bar(bar)

        # Act
        self.cache.add_bars([bar])
        result = self.cache.bars(bar.bar_type)

        # Assert
        assert result == [bar]

    def test_instrument_when_no_instrument_returns_none(self):
        # Arrange, Act
        result = self.cache.instrument(AUDUSD_SIM.id)

        # Assert
        assert result is None

    def test_instrument_when_instrument_exists_returns_expected(self):
        # Arrange
        self.cache.add_instrument(AUDUSD_SIM)

        # Act
        result = self.cache.instrument(AUDUSD_SIM.id)

        # Assert
        assert result == AUDUSD_SIM

    def test_order_book_when_order_book_exists_returns_expected(self):
        # Arrange
        snapshot = OrderBookSnapshot(
            instrument_id=ETHUSDT_BINANCE.id,
            book_type=BookType.L2_MBP,
            bids=[[1550.15, 0.51], [1580.00, 1.20]],
            asks=[[1552.15, 1.51], [1582.00, 2.20]],
            ts_event=0,
            ts_init=0,
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
        assert result == order_book

    def test_price_when_no_ticks_returns_none(self):
        # Act
        result = self.cache.price(AUDUSD_SIM.id, PriceType.LAST)

        # Assert
        assert result is None

    def test_price_given_last_when_no_trade_ticks_returns_none(self):
        # Act
        tick = TestDataStubs.quote_tick_5decimal()

        self.cache.add_quote_tick(tick)

        result = self.cache.price(AUDUSD_SIM.id, PriceType.LAST)

        # Assert
        assert result is None

    def test_price_given_quote_price_type_when_no_quote_ticks_returns_none(self):
        # Arrange
        tick = TestDataStubs.trade_tick_5decimal()

        self.cache.add_trade_tick(tick)

        # Act
        result = self.cache.price(AUDUSD_SIM.id, PriceType.MID)

        # Assert
        assert result is None

    def test_price_given_last_when_trade_tick_returns_expected_price(self):
        # Arrange
        tick = TestDataStubs.trade_tick_5decimal()

        self.cache.add_trade_tick(tick)

        # Act
        result = self.cache.price(AUDUSD_SIM.id, PriceType.LAST)

        # Assert
        assert result == Price.from_str("1.00001")

    @pytest.mark.parametrize(
        "price_type, expected",
        [
            [PriceType.BID, Price.from_str("1.00001")],
            [PriceType.ASK, Price.from_str("1.00003")],
            [PriceType.MID, Price.from_str("1.000020")],
        ],
    )
    def test_price_given_various_quote_price_types_when_quote_tick_returns_expected_price(
        self, price_type, expected
    ):
        # Arrange
        tick = TestDataStubs.quote_tick_5decimal()

        self.cache.add_quote_tick(tick)

        # Act
        result = self.cache.price(AUDUSD_SIM.id, price_type)

        # Assert
        assert result == expected

    def test_quote_tick_when_index_out_of_range_returns_none(self):
        # Arrange
        tick = TestDataStubs.quote_tick_5decimal()

        self.cache.add_quote_tick(tick)

        # Act
        result = self.cache.quote_tick(AUDUSD_SIM.id, index=1)

        # Assert
        assert self.cache.quote_tick_count(AUDUSD_SIM.id) == 1
        assert result is None

    def test_quote_tick_with_two_ticks_returns_expected_tick(self):
        # Arrange
        tick1 = TestDataStubs.quote_tick_5decimal()
        tick2 = TestDataStubs.quote_tick_5decimal()

        self.cache.add_quote_tick(tick1)
        self.cache.add_quote_tick(tick2)

        # Act
        result = self.cache.quote_tick(AUDUSD_SIM.id, index=0)

        # Assert
        assert self.cache.quote_tick_count(AUDUSD_SIM.id) == 2
        assert result == tick2

    def test_trade_tick_when_index_out_of_range_returns_none(self):
        # Arrange
        tick = TestDataStubs.trade_tick_5decimal()

        self.cache.add_trade_tick(tick)

        # Act
        result = self.cache.trade_tick(AUDUSD_SIM.id, index=1)

        # Assert
        assert self.cache.trade_tick_count(AUDUSD_SIM.id) == 1
        assert result is None

    def test_trade_tick_with_one_tick_returns_expected_tick(self):
        # Arrange
        tick1 = TestDataStubs.trade_tick_5decimal()
        tick2 = TestDataStubs.trade_tick_5decimal()

        self.cache.add_trade_tick(tick1)
        self.cache.add_trade_tick(tick2)

        # Act
        result = self.cache.trade_tick(AUDUSD_SIM.id, index=0)

        # Assert
        assert self.cache.trade_tick_count(AUDUSD_SIM.id) == 2
        assert result == tick2

    def test_bar_index_out_of_range_returns_expected_bar(self):
        # Arrange
        bar = TestDataStubs.bar_5decimal()

        self.cache.add_bar(bar)

        # Act
        result = self.cache.bar(bar.bar_type, index=1)

        # Assert
        assert self.cache.bar_count(bar.bar_type) == 1
        assert result is None

    def test_bar_with_two_bars_returns_expected_bar(self):
        # Arrange
        bar_type = TestDataStubs.bartype_audusd_1min_bid()
        bar1 = TestDataStubs.bar_5decimal()
        bar2 = TestDataStubs.bar_5decimal()

        self.cache.add_bar(bar1)
        self.cache.add_bar(bar2)

        # Act
        result = self.cache.bar(bar_type, index=0)

        # Assert
        assert self.cache.bar_count(bar_type) == 2
        assert result == bar2

    def test_get_xrate_returns_correct_rate(self):
        # Arrange
        self.cache.add_instrument(USDJPY_SIM)

        tick = QuoteTick(
            instrument_id=USDJPY_SIM.id,
            bid=Price.from_str("110.80000"),
            ask=Price.from_str("110.80010"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        self.cache.add_quote_tick(tick)

        # Act
        result = self.cache.get_xrate(SIM, JPY, USD)

        # Assert
        assert result == 0.009025266685348969

    def test_get_xrate_with_no_conversion_returns_one(self):
        # Arrange, Act
        result = self.cache.get_xrate(SIM, AUD, AUD)

        # Assert
        assert result == Decimal("1")

    def test_get_xrate_with_conversion(self):
        # Arrange
        self.cache.add_instrument(AUDUSD_SIM)

        tick = QuoteTick(
            instrument_id=AUDUSD_SIM.id,
            bid=Price.from_str("0.80000"),
            ask=Price.from_str("0.80010"),
            bid_size=Quantity.from_int(1),
            ask_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        self.cache.add_quote_tick(tick)

        # Act
        result = self.cache.get_xrate(SIM, AUD, USD)

        # Assert
        assert result == 0.80005
