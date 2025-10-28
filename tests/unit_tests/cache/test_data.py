# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.rust.model import AggregationSource
from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.currencies import EUR
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.currencies import JPY
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import FundingRateUpdate
from nautilus_trader.model.data import MarkPriceUpdate
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs


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

    def test_own_order_book_for_unknown_instrument_returns_none(self):
        # Arrange, Act, Assert
        assert self.cache.own_order_book(AUDUSD_SIM.id) is None

    def test_audit_own_order_books_with_no_orders(self):
        # Arrange, Act, Assert
        self.cache.audit_own_order_books()  # Should not raise

    def test_update_own_order_book_with_market_order_does_not_raise(self):
        # Arrange
        from nautilus_trader.model.enums import OrderSide
        from nautilus_trader.test_kit.stubs.component import TestComponentStubs
        from nautilus_trader.test_kit.stubs.events import TestEventStubs

        order_factory = TestComponentStubs.order_factory()

        # First, create a LIMIT order to establish an own book for the instrument
        limit_order = order_factory.limit(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100),
            price=Price.from_str("1.00000"),
        )
        self.cache.add_order(limit_order)
        self.cache.update_own_order_book(limit_order)

        # Verify own book now exists
        assert self.cache.own_order_book(AUDUSD_SIM.id) is not None

        # Create a MARKET order (no price) and apply events to close it
        market_order = order_factory.market(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(50),
        )
        self.cache.add_order(market_order)

        # Transition market order through valid states: INITIALIZED -> SUBMITTED -> ACCEPTED -> FILLED
        submitted = TestEventStubs.order_submitted(market_order)
        market_order.apply(submitted)

        accepted = TestEventStubs.order_accepted(market_order)
        market_order.apply(accepted)

        fill = TestEventStubs.order_filled(market_order, instrument=AUDUSD_SIM)
        market_order.apply(fill)

        # Act: update_own_order_book with closed MARKET order should gracefully skip
        # Previously this raised: TypeError: Cannot initialize MARKET order as `nautilus_pyo3.OwnBookOrder`, no price
        # The bug occurred because the bypass (own_book is not None and order.is_closed_c()) allowed
        # MARKET orders through, then to_own_book_order() raised TypeError
        self.cache.update_own_order_book(market_order)  # Should not raise

        # Assert: own order book still exists (from limit order) but market order not added
        assert self.cache.own_order_book(AUDUSD_SIM.id) is not None

    def test_force_remove_from_own_order_book_cleans_up_indexes(self):
        from nautilus_trader.model.enums import OrderSide
        from nautilus_trader.test_kit.stubs.component import TestComponentStubs
        from nautilus_trader.test_kit.stubs.events import TestEventStubs

        order_factory = TestComponentStubs.order_factory()

        # Create and add a limit order
        limit_order = order_factory.limit(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100),
            price=Price.from_str("1.00000"),
        )
        self.cache.add_order(limit_order)
        self.cache.update_own_order_book(limit_order)

        # Transition to SUBMITTED (inflight)
        submitted = TestEventStubs.order_submitted(limit_order)
        limit_order.apply(submitted)
        self.cache.update_order(limit_order)

        # Verify order is in inflight index
        assert limit_order in self.cache.orders_inflight()
        assert self.cache.own_order_book(AUDUSD_SIM.id) is not None

        # Force remove the order
        self.cache.force_remove_from_own_order_book(limit_order.client_order_id)

        # Assert all indexes are cleaned up
        assert limit_order not in self.cache.orders_open()
        assert limit_order not in self.cache.orders_inflight()
        assert limit_order not in self.cache.orders_emulated()
        assert not self.cache.is_order_pending_cancel_local(limit_order.client_order_id)
        assert limit_order in self.cache.orders_closed()

    def test_audit_own_order_books_preserves_inflight_orders(self):
        from nautilus_trader.model.enums import OrderSide
        from nautilus_trader.test_kit.stubs.component import TestComponentStubs
        from nautilus_trader.test_kit.stubs.events import TestEventStubs

        order_factory = TestComponentStubs.order_factory()

        # Create and add a limit order
        limit_order = order_factory.limit(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100),
            price=Price.from_str("1.00000"),
        )
        self.cache.add_order(limit_order)
        self.cache.update_own_order_book(limit_order)

        # Transition to SUBMITTED (inflight)
        submitted = TestEventStubs.order_submitted(limit_order)
        limit_order.apply(submitted)
        self.cache.update_order(limit_order)

        # Verify own book has the order
        own_book = self.cache.own_order_book(AUDUSD_SIM.id)
        assert own_book is not None
        assert len(own_book.bids_to_list()) > 0

        # Run audit - should NOT remove inflight orders
        self.cache.audit_own_order_books()

        # Assert order still in own book
        own_book = self.cache.own_order_book(AUDUSD_SIM.id)
        assert own_book is not None
        assert len(own_book.bids_to_list()) > 0

    def test_audit_own_order_books_removes_closed_orders(self):
        from nautilus_trader.model.enums import OrderSide
        from nautilus_trader.test_kit.stubs.component import TestComponentStubs
        from nautilus_trader.test_kit.stubs.events import TestEventStubs

        order_factory = TestComponentStubs.order_factory()

        # Create and add a limit order
        limit_order = order_factory.limit(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100),
            price=Price.from_str("1.00000"),
        )
        self.cache.add_order(limit_order)
        self.cache.update_own_order_book(limit_order)

        # Transition through states to ACCEPTED
        submitted = TestEventStubs.order_submitted(limit_order)
        limit_order.apply(submitted)
        self.cache.update_order(limit_order)

        accepted = TestEventStubs.order_accepted(limit_order)
        limit_order.apply(accepted)
        self.cache.update_order(limit_order)

        # Verify own book has the order
        own_book = self.cache.own_order_book(AUDUSD_SIM.id)
        assert own_book is not None
        assert len(own_book.bids_to_list()) > 0

        # Cancel the order (transition to closed)
        canceled = TestEventStubs.order_canceled(limit_order)
        limit_order.apply(canceled)
        self.cache.update_order(limit_order)

        # Manually add to own book to simulate stale state
        self.cache.update_own_order_book(limit_order)

        # Run audit - should remove closed orders
        self.cache.audit_own_order_books()

        # Assert order removed from own book
        own_book = self.cache.own_order_book(AUDUSD_SIM.id)
        assert own_book is not None
        assert len(own_book.bids_to_list()) == 0

    @pytest.mark.parametrize(
        ("price_type"),
        [
            PriceType.BID,
            PriceType.ASK,
            PriceType.MID,
            PriceType.LAST,
            PriceType.MARK,
        ],
    )
    def test_price_when_no_prices_returns_none(self, price_type: PriceType):
        # Arrange, Act, Assert
        assert self.cache.price(AUDUSD_SIM.id, price_type) is None

    @pytest.mark.parametrize(
        ("price_type"),
        [
            PriceType.BID,
            PriceType.ASK,
            PriceType.MID,
            PriceType.LAST,
            PriceType.MARK,
        ],
    )
    def test_prices_when_no_prices_returns_empty_map(self, price_type: PriceType):
        # Arrange, Act, Assert
        assert self.cache.prices(price_type) == {}

    def test_quote_tick_when_no_ticks_returns_none(self):
        # Arrange, Act, Assert
        assert self.cache.quote_tick(AUDUSD_SIM.id) is None

    def test_trade_tick_when_no_ticks_returns_none(self):
        # Arrange, Act, Assert
        assert self.cache.trade_tick(AUDUSD_SIM.id) is None

    def test_bar_when_no_bars_returns_none(self):
        # Arrange, Act, Assert
        assert self.cache.bar(TestDataStubs.bartype_gbpusd_1sec_mid()) is None

    def test_quote_tick_count_for_unknown_instrument_returns_zero(self):
        # Arrange, Act, Assert
        assert self.cache.quote_tick_count(AUDUSD_SIM.id) == 0

    def test_trade_tick_count_for_unknown_instrument_returns_zero(self):
        # Arrange, Act, Assert
        assert self.cache.trade_tick_count(AUDUSD_SIM.id) == 0

    def test_has_order_book_for_unknown_instrument_returns_false(self):
        # Arrange, Act, Assert
        assert not self.cache.has_order_book(AUDUSD_SIM.id)

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

    def test_instruments_with_underlying_returns_expected(self):
        # Arrange
        instrument1 = TestInstrumentProvider.future(symbol="ESZ24", underlying="ES")
        instrument2 = TestInstrumentProvider.future(symbol="CLZ24", underlying="CL")

        self.cache.add_instrument(instrument1)
        self.cache.add_instrument(instrument2)

        # Act
        result = self.cache.instruments(underlying="ES")

        # Assert
        assert result == [instrument1]

    def test_synthetic_ids_when_one_synthetic_instrument_returns_expected_list(self):
        # Arrange
        synthetic = TestInstrumentProvider.synthetic_instrument()

        self.cache.add_synthetic(synthetic)

        # Act
        result = self.cache.synthetic_ids()

        # Assert
        assert result == [synthetic.id]

    def test_synthetics_when_one_synthetic_instrument_returns_expected_list(self):
        # Arrange
        synthetic = TestInstrumentProvider.synthetic_instrument()

        self.cache.add_synthetic(synthetic)

        # Act
        result = self.cache.synthetics()

        # Assert
        assert result == [synthetic]

    def test_add_mark_price(self):
        # Arrange
        instrument_id = InstrumentId.from_str("ETH-USD-SWAP.OKX")
        value = Price(10_000, 2)
        mark_price = MarkPriceUpdate(
            instrument_id=instrument_id,
            value=value,
            ts_event=0,
            ts_init=0,
        )

        self.cache.add_mark_price(mark_price)

        # Act
        result = self.cache.price(instrument_id, PriceType.MARK)

        # Assert
        assert result == value

    def test_add_mark_price_as_map(self):
        # Arrange
        instrument_id = InstrumentId.from_str("ETH-USD-SWAP.OKX")
        value = Price(10_000, 2)
        mark_price = MarkPriceUpdate(
            instrument_id=instrument_id,
            value=value,
            ts_event=0,
            ts_init=0,
        )

        self.cache.add_mark_price(mark_price)

        # Act
        result = self.cache.prices(PriceType.MARK)

        # Assert
        assert result == {instrument_id: value}

    def test_add_funding_rate(self):
        # Arrange
        instrument_id = InstrumentId.from_str("ETH-USD-SWAP.OKX")
        rate = Decimal("0.0001")  # 0.01% funding rate
        funding_rate = FundingRateUpdate(
            instrument_id=instrument_id,
            rate=rate,
            ts_event=0,
            ts_init=0,
        )

        self.cache.add_funding_rate(funding_rate)

        # Act
        result = self.cache.funding_rate(instrument_id)

        # Assert
        assert result == funding_rate

    def test_funding_rate_when_no_funding_rate_returns_none(self):
        # Arrange
        instrument_id = InstrumentId.from_str("ETH-USD-SWAP.OKX")

        # Act
        result = self.cache.funding_rate(instrument_id)

        # Assert
        assert result is None

    def test_add_funding_rate_updates_existing(self):
        # Arrange
        instrument_id = InstrumentId.from_str("ETH-USD-SWAP.OKX")

        funding_rate1 = FundingRateUpdate(
            instrument_id=instrument_id,
            rate=Decimal("0.0001"),
            ts_event=0,
            ts_init=0,
        )

        funding_rate2 = FundingRateUpdate(
            instrument_id=instrument_id,
            rate=Decimal("0.0002"),
            ts_event=1,
            ts_init=1,
        )

        # Act
        self.cache.add_funding_rate(funding_rate1)
        self.cache.add_funding_rate(funding_rate2)
        result = self.cache.funding_rate(instrument_id)

        # Assert
        assert result == funding_rate2
        assert result.rate == Decimal("0.0002")

    def test_reset_clears_funding_rates(self):
        # Arrange
        instrument_id = InstrumentId.from_str("ETH-USD-SWAP.OKX")
        funding_rate = FundingRateUpdate(
            instrument_id=instrument_id,
            rate=Decimal("0.0001"),
            ts_event=0,
            ts_init=0,
        )
        self.cache.add_funding_rate(funding_rate)

        # Act
        self.cache.reset()

        # Assert
        assert self.cache.funding_rate(instrument_id) is None

    def test_quote_ticks_when_one_tick_returns_expected_list(self):
        # Arrange
        tick = TestDataStubs.quote_tick()

        self.cache.add_quote_ticks([tick])

        # Act
        result = self.cache.quote_ticks(tick.instrument_id)

        # Assert
        assert result == [tick]

    def test_add_quote_ticks_when_identical_ticks_does_not_add(self):
        # Arrange
        tick = TestDataStubs.quote_tick()

        self.cache.add_quote_tick(tick)

        # Act
        self.cache.add_quote_ticks([tick])
        result = self.cache.quote_ticks(tick.instrument_id)

        # Assert
        assert result == [tick]

    def test_add_quote_ticks_when_older_quotes(self):
        # Arrange
        tick1 = TestDataStubs.quote_tick()
        self.cache.add_quote_tick(tick1)

        tick2 = TestDataStubs.quote_tick(ts_event=1, ts_init=1)

        # Act
        self.cache.add_quote_ticks([tick2])
        result = self.cache.quote_ticks(tick1.instrument_id)

        # Assert
        assert result == [tick2, tick1]

    def test_trade_ticks_when_one_tick_returns_expected_list(self):
        # Arrange
        tick = TestDataStubs.trade_tick()

        self.cache.add_trade_ticks([tick])

        # Act
        result = self.cache.trade_ticks(tick.instrument_id)

        # Assert
        assert result == [tick]

    def test_add_trade_ticks_when_identical_ticks_does_not_add(self):
        # Arrange
        tick = TestDataStubs.trade_tick()

        self.cache.add_trade_tick(tick)

        # Act
        self.cache.add_trade_ticks([tick])
        result = self.cache.trade_ticks(tick.instrument_id)

        # Assert
        assert result == [tick]

    def test_add_trade_ticks_when_older_trades(self):
        # Arrange
        tick1 = TestDataStubs.trade_tick()
        self.cache.add_trade_tick(tick1)

        tick2 = TestDataStubs.trade_tick(ts_event=1, ts_init=1)
        self.cache.add_trade_tick(tick2)

        # Act
        self.cache.add_trade_ticks([tick1])
        result = self.cache.trade_ticks(tick1.instrument_id)

        # Assert
        assert result == [tick2, tick1]

    def test_bars_when_one_bar_returns_expected_list(self):
        # Arrange
        bar = TestDataStubs.bar_5decimal()

        self.cache.add_bars([bar])

        # Act
        result = self.cache.bars(bar.bar_type)

        # Assert
        assert result == [bar]

    def test_add_bars_when_already_identical_bar_does_not_add(self):
        # Arrange
        bar = TestDataStubs.bar_5decimal()

        self.cache.add_bar(bar)

        # Act
        self.cache.add_bars([bar])
        result = self.cache.bars(bar.bar_type)

        # Assert
        assert result == [bar]

    def test_add_bars_when_older_cached_bars(self):
        # Arrange
        bar1 = TestDataStubs.bar_5decimal()
        self.cache.add_bar(bar1)

        bar2 = TestDataStubs.bar_5decimal(ts_event=1)
        self.cache.add_bar(bar2)

        # Act
        self.cache.add_bars([bar2])
        result = self.cache.bars(bar1.bar_type)

        # Assert
        assert result == [bar2, bar1]

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
        instrument = ETHUSDT_BINANCE
        order_book = TestDataStubs.order_book(instrument)
        self.cache.add_order_book(order_book)

        # Act
        result = self.cache.order_book(instrument.id)

        # Assert
        assert result == order_book

    def test_own_order_book_when_order_book_exists_returns_expected(self):
        # Arrange
        instrument = ETHUSDT_BINANCE
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(instrument.id.value)
        pyo3_own_order_book = nautilus_pyo3.OwnOrderBook(pyo3_instrument_id)
        self.cache.add_own_order_book(pyo3_own_order_book)

        # Act
        result = self.cache.own_order_book(instrument.id)

        # Assert
        assert result == pyo3_own_order_book

    def test_price_when_no_ticks_returns_none(self):
        # Act
        result = self.cache.price(AUDUSD_SIM.id, PriceType.LAST)

        # Assert
        assert result is None

    def test_price_given_last_when_no_trade_ticks_returns_none(self):
        # Act
        tick = TestDataStubs.quote_tick()

        self.cache.add_quote_tick(tick)

        result = self.cache.price(AUDUSD_SIM.id, PriceType.LAST)

        # Assert
        assert result is None

    def test_price_given_quote_price_type_when_no_quote_ticks_returns_none(self):
        # Arrange
        tick = TestDataStubs.trade_tick()

        self.cache.add_trade_tick(tick)

        # Act
        result = self.cache.price(AUDUSD_SIM.id, PriceType.MID)

        # Assert
        assert result is None

    def test_price_given_last_when_trade_tick_returns_expected_price(self):
        # Arrange
        tick = TestDataStubs.trade_tick()

        self.cache.add_trade_tick(tick)

        # Act
        result = self.cache.price(AUDUSD_SIM.id, PriceType.LAST)

        # Assert
        assert result == Price.from_str("1.00000")

    def test_prices_given_last_when_trade_tick(self):
        # Arrange
        tick = TestDataStubs.trade_tick()

        self.cache.add_trade_tick(tick)

        # Act
        result = self.cache.prices(PriceType.LAST)

        # Assert
        assert result == {AUDUSD_SIM.id: Price.from_str("1.00000")}

    @pytest.mark.parametrize(
        ("price_type", "expected"),
        [
            [PriceType.BID, Price.from_str("1.00001")],
            [PriceType.ASK, Price.from_str("1.00003")],
            [PriceType.MID, Price.from_str("1.000020")],
        ],
    )
    def test_price_given_various_quote_price_types_when_quote_tick_returns_expected_price(
        self,
        price_type,
        expected,
    ):
        # Arrange
        tick = TestDataStubs.quote_tick(
            instrument=AUDUSD_SIM,
            bid_price=1.00001,
            ask_price=1.00003,
        )

        self.cache.add_quote_tick(tick)

        # Act
        result = self.cache.price(AUDUSD_SIM.id, price_type)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("price_type", "expected"),
        [
            [PriceType.BID, Price.from_str("1.00001")],
            [PriceType.ASK, Price.from_str("1.00003")],
            [PriceType.MID, Price.from_str("1.000020")],
        ],
    )
    def test_prices_given_various_quote_price_types(
        self,
        price_type,
        expected,
    ):
        # Arrange
        tick = TestDataStubs.quote_tick(
            instrument=AUDUSD_SIM,
            bid_price=1.00001,
            ask_price=1.00003,
        )

        self.cache.add_quote_tick(tick)

        # Act
        result = self.cache.prices(price_type)

        # Assert
        assert result == {AUDUSD_SIM.id: expected}

    @pytest.mark.parametrize(
        ("price_type", "expected"),
        [[PriceType.BID, Price.from_str("1.00003")], [PriceType.LAST, None]],
    )
    def test_price_returned_with_external_bars(self, price_type, expected):
        # Arrange
        self.cache.add_bar(TestDataStubs.bar_5decimal())
        self.cache.add_bar(TestDataStubs.bar_5decimal_5min_bid())
        self.cache.add_bar(TestDataStubs.bar_3decimal())

        # Act
        result = self.cache.price(AUDUSD_SIM.id, price_type)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("instrument_id", "price_type", "aggregation_source", "expected"),
        [
            [
                AUDUSD_SIM.id,
                None,
                None,
                [
                    TestDataStubs.bartype_audusd_1min_bid(),
                    TestDataStubs.bartype_audusd_5min_bid(),
                    BarType.from_str("AUD/USD.SIM-1-MONTH-MID-EXTERNAL"),
                ],
            ],
            [
                AUDUSD_SIM.id,
                PriceType.BID,
                None,
                [TestDataStubs.bartype_audusd_1min_bid(), TestDataStubs.bartype_audusd_5min_bid()],
            ],
            [
                AUDUSD_SIM.id,
                PriceType.BID,
                AggregationSource.EXTERNAL,
                [TestDataStubs.bartype_audusd_1min_bid(), TestDataStubs.bartype_audusd_5min_bid()],
            ],
            [AUDUSD_SIM.id, PriceType.ASK, AggregationSource.EXTERNAL, []],
            [ETHUSDT_BINANCE.id, PriceType.BID, AggregationSource.EXTERNAL, []],
        ],
    )
    def test_retrieved_bar_types_match_expected(
        self,
        instrument_id,
        price_type,
        aggregation_source,
        expected,
    ):
        # Arrange
        self.cache.add_bar(TestDataStubs.bar_5decimal())
        self.cache.add_bar(TestDataStubs.bar_5decimal_5min_bid())
        self.cache.add_bar(TestDataStubs.bar_3decimal())
        self.cache.add_bar(TestDataStubs.bar_month_mid())

        # Act
        result = self.cache.bar_types(
            instrument_id=instrument_id,
            price_type=price_type,
            aggregation_source=aggregation_source,
        )

        # Assert
        assert result == expected

    def test_retrieved_all_bar_types_match_expected(self):
        # Arrange
        self.cache.add_bar(TestDataStubs.bar_5decimal())
        self.cache.add_bar(TestDataStubs.bar_5decimal_5min_bid())
        self.cache.add_bar(TestDataStubs.bar_3decimal())

        # Act
        result = self.cache.bar_types()

        # Assert
        assert len(result) == 3

    def test_quote_tick_when_index_out_of_range_returns_none(self):
        # Arrange
        tick = TestDataStubs.quote_tick()

        self.cache.add_quote_tick(tick)

        # Act
        result = self.cache.quote_tick(AUDUSD_SIM.id, index=1)

        # Assert
        assert self.cache.quote_tick_count(AUDUSD_SIM.id) == 1
        assert result is None

    def test_quote_tick_with_two_ticks_returns_expected_tick(self):
        # Arrange
        tick1 = TestDataStubs.quote_tick(ts_init=0)
        tick2 = TestDataStubs.quote_tick(ts_init=1)

        self.cache.add_quote_tick(tick1)
        self.cache.add_quote_tick(tick2)

        # Act
        result = self.cache.quote_tick(AUDUSD_SIM.id, index=0)

        # Assert
        assert self.cache.quote_tick_count(AUDUSD_SIM.id) == 2
        assert result.ts_init == 1

    def test_trade_tick_when_index_out_of_range_returns_none(self):
        # Arrange
        tick = TestDataStubs.trade_tick()

        self.cache.add_trade_tick(tick)

        # Act
        result = self.cache.trade_tick(AUDUSD_SIM.id, index=1)

        # Assert
        assert self.cache.trade_tick_count(AUDUSD_SIM.id) == 1
        assert result is None

    def test_trade_tick_with_one_tick_returns_expected_tick(self):
        # Arrange
        tick1 = TestDataStubs.trade_tick()
        tick2 = TestDataStubs.trade_tick()

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

    def test_get_general_object_when_nothing_in_cache_returns_none(self):
        # Arrange, Act
        result = self.cache.get("something")

        # Assert
        assert result is None

    def test_add_general_object_adds_to_cache(self):
        # Arrange
        key = "value_a"
        obj = b"some string value"

        # Act
        self.cache.add(key, obj)

        # Assert
        assert self.cache.get(key) == obj

    def test_get_xrate_returns_correct_rate(self):
        # Arrange
        self.cache.add_instrument(USDJPY_SIM)

        tick = QuoteTick(
            instrument_id=USDJPY_SIM.id,
            bid_price=Price.from_str("110.80000"),
            ask_price=Price.from_str("110.80010"),
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
            bid_price=Price.from_str("0.80000"),
            ask_price=Price.from_str("0.80010"),
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

    def test_get_xrate_fallbacks_to_bars_if_no_quotes_returns_correct_rate(self):
        # Arrange
        self.cache.reset()
        self.cache.add_instrument(AUDUSD_SIM)

        bid_price = Price.from_str("0.80000")
        bid_bar = Bar(
            bar_type=BarType.from_str(f"{AUDUSD_SIM.id}-1-DAY-BID-EXTERNAL"),
            open=bid_price,
            high=bid_price,
            low=bid_price,
            close=bid_price,
            volume=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        ask_price = Price.from_str("0.80010")
        ask_bar = Bar(
            bar_type=BarType.from_str(f"{AUDUSD_SIM.id}-1-DAY-ASK-EXTERNAL"),
            open=ask_price,
            high=ask_price,
            low=ask_price,
            close=ask_price,
            volume=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        self.cache.add_bar(bid_bar)
        self.cache.add_bar(ask_bar)

        # Act
        result = self.cache.get_xrate(SIM, AUD, USD)

        # Assert
        assert result == 0.80005

    def test_get_xrate_fallbacks_to_bars_if_no_quotes_returns_correct_rate_with_add_bars(self):
        # Arrange
        self.cache.reset()
        self.cache.add_instrument(AUDUSD_SIM)
        bid_price = Price.from_str("0.80000")
        ask_price = Price.from_str("0.80010")

        bid_bar1 = Bar(
            bar_type=BarType.from_str(f"{AUDUSD_SIM.id}-1-DAY-BID-EXTERNAL"),
            open=bid_price,
            high=bid_price,
            low=bid_price,
            close=bid_price,
            volume=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )
        bid_bar2 = Bar(
            bar_type=BarType.from_str(f"{AUDUSD_SIM.id}-1-DAY-BID-EXTERNAL"),
            open=Price.from_str("0"),
            high=Price.from_str("0"),
            low=Price.from_str("0"),
            close=Price.from_str("0"),
            volume=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        ask_bar1 = Bar(
            bar_type=BarType.from_str(f"{AUDUSD_SIM.id}-1-DAY-ASK-EXTERNAL"),
            open=ask_price,
            high=ask_price,
            low=ask_price,
            close=ask_price,
            volume=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )
        ask_bar2 = Bar(
            bar_type=BarType.from_str(f"{AUDUSD_SIM.id}-1-DAY-ASK-EXTERNAL"),
            open=Price.from_str("0"),
            high=Price.from_str("0"),
            low=Price.from_str("0"),
            close=Price.from_str("0"),
            volume=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        self.cache.add_bars([bid_bar2, bid_bar1])
        self.cache.add_bars([ask_bar2, ask_bar1])

        # Act
        result = self.cache.get_xrate(SIM, AUD, USD)

        # Assert
        assert result == 0.80005

    def test_get_mark_xrate_returns_none_when_not_set(self):
        """
        When no mark exchange rate is set for a currency pair, get_mark_xrate should
        return None.
        """
        result = self.cache.get_mark_xrate(USD, EUR)
        assert result is None

    def test_set_and_get_mark_xrate(self):
        """
        After setting a mark exchange rate, get_mark_xrate should return the correct
        value in both the forward and inverse directions.
        """
        xrate = 1.25
        self.cache.set_mark_xrate(USD, EUR, xrate)

        forward = self.cache.get_mark_xrate(USD, EUR)
        inverse = self.cache.get_mark_xrate(EUR, USD)

        assert forward == xrate
        assert inverse == 1.0 / xrate

    def test_clear_mark_xrate(self):
        """
        Clearing a mark exchange rate for a specific pair should remove the forward rate
        while leaving the inverse rate intact.
        """
        xrate = 1.25
        self.cache.set_mark_xrate(USD, EUR, xrate)
        # Precondition: both forward and inverse rates are set
        assert self.cache.get_mark_xrate(USD, EUR) is not None
        assert self.cache.get_mark_xrate(EUR, USD) is not None

        # Act: clear the forward rate
        self.cache.clear_mark_xrate(USD, EUR)

        # Assert: forward rate is removed but the inverse remains
        assert self.cache.get_mark_xrate(USD, EUR) is None
        assert self.cache.get_mark_xrate(EUR, USD) == 1.0 / xrate

    def test_clear_mark_xrates(self):
        """
        Clearing all mark exchange rates should remove every rate.
        """
        self.cache.set_mark_xrate(USD, EUR, 1.25)
        self.cache.set_mark_xrate(GBP, USD, 1.40)
        # Precondition: verify that both directions exist
        assert self.cache.get_mark_xrate(USD, EUR) is not None
        assert self.cache.get_mark_xrate(EUR, USD) is not None
        assert self.cache.get_mark_xrate(GBP, USD) is not None
        assert self.cache.get_mark_xrate(USD, GBP) is not None

        # Act: clear all mark exchange rates
        self.cache.clear_mark_xrates()

        # Assert: every mark exchange rate should be cleared
        assert self.cache.get_mark_xrate(USD, EUR) is None
        assert self.cache.get_mark_xrate(EUR, USD) is None
        assert self.cache.get_mark_xrate(GBP, USD) is None
        assert self.cache.get_mark_xrate(USD, GBP) is None

    def test_set_mark_xrate_zero_raises(self):
        """
        Setting a mark exchange rate of zero should raise a ValueError to avoid
        division-by-zero.
        """
        import pytest

        with pytest.raises(ValueError):
            self.cache.set_mark_xrate(USD, EUR, 0.0)
